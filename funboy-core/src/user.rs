use std::{
    hash::Hash,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use moka::future::{Cache, CacheBuilder};
use tokio::sync::Mutex;

use crate::{
    Request,
    database::{FunboyDatabase, Platform},
    ollama::OllamaSettings,
    permissions::{Permission, PermissionError, Permissions, Role},
};

#[derive(Debug, Clone)]
pub struct UserCtx {
    pub is_generating: Arc<AtomicBool>,
    pub ollama_settings: Arc<Mutex<OllamaSettings>>,
    pub pending_requests: Arc<Mutex<Vec<Request>>>,
    permissions: Arc<Mutex<Permissions>>,
}

impl Default for UserCtx {
    fn default() -> Self {
        Self {
            is_generating: Default::default(),
            ollama_settings: Default::default(),
            pending_requests: Default::default(),
            permissions: Default::default(),
        }
    }
}

impl UserCtx {
    pub fn new() -> UserCtx {
        Self {
            is_generating: Arc::new(AtomicBool::new(false)),
            ..Default::default()
        }
    }

    pub fn with_permissions(mut self, permissions: Permissions) -> UserCtx {
        self.permissions = Arc::new(Mutex::new(permissions));
        self
    }

    pub fn with_ollama_settings(mut self, settings: OllamaSettings) -> UserCtx {
        self.ollama_settings = Arc::new(Mutex::new(settings));
        self
    }
}

pub struct FlagGuard {
    flag: Arc<AtomicBool>,
}

impl FlagGuard {
    pub fn new(flag: Arc<AtomicBool>) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .ok()
            .map(|_| Self { flag })
    }
}

impl Drop for FlagGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Relaxed);
    }
}

pub trait FunboyUserId:
    std::fmt::Debug + Eq + Hash + Send + Sync + Clone + ToString + 'static
{
}

#[derive(Debug, Clone)]
pub struct UserMap<U: FunboyUserId> {
    platform: Platform,
    db: FunboyDatabase,
    users: Cache<U, UserCtx>,
}

impl<U: FunboyUserId> UserMap<U> {
    pub fn new(platform: Platform, db: FunboyDatabase) -> Self {
        Self {
            platform,
            db,
            users: CacheBuilder::new(100_000)
                .time_to_idle(Duration::from_secs(60 * 60 * 24))
                .build(),
        }
    }

    pub async fn get_or_insert(&self, user_id: U) -> UserCtx {
        if let Some(user_ctx) = self.users.get(&user_id).await {
            user_ctx
        } else {
            let user = match self
                .db
                .create_user(self.platform, user_id.to_string())
                .await
            {
                Ok(user) => user,
                Err(e) => {
                    eprintln!("{e}");
                    return UserCtx::default();
                }
            };

            let permissions = match self.db.get_permissions(user.id).await {
                Ok(permissions) => permissions,
                Err(e) => {
                    eprintln!("{e}");
                    return UserCtx::default();
                }
            };

            let ollama_settings = match self.db.get_ollama_settings(user.id).await {
                Ok(ollama_settings) => ollama_settings,
                Err(e) => {
                    eprintln!("{e}");
                    return UserCtx::default();
                }
            };

            let user_ctx = UserCtx::default()
                .with_permissions(permissions)
                .with_ollama_settings(ollama_settings);

            self.users.get_with(user_id, async { user_ctx }).await
        }
    }

    pub async fn update_ollama_settings(
        &self,
        user_id: U,
        platform: Platform,
        settings: OllamaSettings,
    ) {
        let user_ctx = self.get_or_insert(user_id.clone()).await;
        let mut ollama_settings = user_ctx.ollama_settings.lock().await;
        *ollama_settings = settings;

        match self.db.get_user_id(&user_id.to_string(), platform).await {
            Ok(user_id) => {
                let result = self
                    .db
                    .update_ollama_settings(user_id, ollama_settings.clone())
                    .await;
                if let Err(e) = result {
                    eprintln!("{}", e.to_string())
                }
            }
            Err(e) => {
                eprintln!("{}", e.to_string())
            }
        }
    }

    pub async fn grant_all_permissions(&mut self, user_id: U) {
        let user_ctx = self.get_or_insert(user_id.clone()).await;

        let mut permissions = user_ctx.permissions.lock().await;
        permissions.0 = Permissions::owner().0;

        let result = self
            .db
            .update_permissions(self.platform, user_id.to_string(), &Permissions::owner())
            .await;

        if let Err(e) = result {
            eprintln!("{}", e.to_string())
        }
    }

    pub async fn set_permissions(
        &mut self,
        user_id: U,
        permissions: Permissions,
    ) -> Result<(), PermissionError> {
        let user_ctx = self.get_or_insert(user_id.clone()).await;

        let mut current_permissions = user_ctx.permissions.lock().await;

        if current_permissions.is_owner() {
            return Err(PermissionError::CannotChangeOwnersRole);
        }

        *current_permissions = permissions;

        let result = self
            .db
            .update_permissions(self.platform, user_id.to_string(), &current_permissions)
            .await;

        if let Err(e) = result {
            eprintln!("{}", e.to_string())
        }

        Ok(())
    }

    pub async fn grant_permissions(
        &mut self,
        user_id: U,
        permissions: &[Permission],
    ) -> Result<(), PermissionError> {
        if permissions.contains(&Permission::Owner) {
            return Err(PermissionError::CannotGrantOwnerPermission);
        }

        let user_ctx = self.get_or_insert(user_id.clone()).await;

        let mut current_permissions = user_ctx.permissions.lock().await;
        for permission in permissions {
            current_permissions.0.insert(*permission);
        }

        let result = self
            .db
            .update_permissions(self.platform, user_id.to_string(), &current_permissions)
            .await;

        if let Err(e) = result {
            eprintln!("{}", e.to_string())
        }

        Ok(())
    }

    pub async fn revoke_permissions(
        &mut self,
        user_id: U,
        permissions: &[Permission],
    ) -> Result<(), PermissionError> {
        if permissions.contains(&Permission::Owner) {
            return Err(PermissionError::CannotRevokeOwnerPermission);
        }

        let user_ctx = self.get_or_insert(user_id.clone()).await;
        let mut current_permissions = user_ctx.permissions.lock().await;

        if current_permissions.is_owner() {
            return Err(PermissionError::CannotRevokePermissionsFromOwner);
        }

        for permission in permissions {
            current_permissions.0.remove(permission);
        }

        let result = self
            .db
            .update_permissions(self.platform, user_id.to_string(), &current_permissions)
            .await;

        if let Err(e) = result {
            eprintln!("{}", e.to_string())
        }

        Ok(())
    }

    pub async fn set_role(&mut self, user_id: U, role: Role) -> Result<(), PermissionError> {
        self.set_permissions(user_id, role.into()).await
    }

    pub async fn get_permissions(&self, user_id: U) -> Permissions {
        let user_ctx = self.get_or_insert(user_id.clone()).await;
        let permissions = user_ctx.permissions.lock().await;
        permissions.clone()
    }
}
