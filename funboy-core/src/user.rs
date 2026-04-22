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
    FunboyError, Request,
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

    pub async fn get_or_insert(&self, user_id: U) -> Result<UserCtx, FunboyError> {
        self.users
            .try_get_with(user_id.clone(), async move {
                match self
                    .db
                    .insert_user(self.platform, user_id.to_string())
                    .await?
                {
                    (user, true) => {
                        self.db
                            .update_permissions(
                                self.platform,
                                user_id.to_string(),
                                &Permissions::default(),
                            )
                            .await?;
                        self.db
                            .update_ollama_settings(user.id, OllamaSettings::default())
                            .await?;
                        Ok(UserCtx::default())
                    }
                    (user, false) => {
                        let permissions = self.db.get_permissions(user.id).await?;
                        let ollama_settings = self.db.get_ollama_settings(user.id).await?;

                        let user_ctx = UserCtx::default()
                            .with_permissions(permissions)
                            .with_ollama_settings(ollama_settings);

                        Ok(user_ctx)
                    }
                }
            })
            .await
            .map_err(|e: Arc<FunboyError>| (*e).clone())
    }

    pub async fn update_ollama_settings(
        &self,
        user_id: U,
        platform: Platform,
        settings: OllamaSettings,
    ) -> Result<(), FunboyError> {
        let user_ctx = self.get_or_insert(user_id.clone()).await?;
        let mut ollama_settings = user_ctx.ollama_settings.lock().await;
        *ollama_settings = settings;

        let user_id = self.db.get_user_id(&user_id.to_string(), platform).await?;
        self.db
            .update_ollama_settings(user_id, ollama_settings.clone())
            .await?;
        Ok(())
    }

    pub async fn grant_all_permissions(&self, user_id: U) -> Result<(), FunboyError> {
        let user_ctx = self.get_or_insert(user_id.clone()).await?;

        let mut permissions = user_ctx.permissions.lock().await;
        permissions.0 = Permissions::owner().0;

        self.db
            .update_permissions(self.platform, user_id.to_string(), &Permissions::owner())
            .await?;

        Ok(())
    }

    pub async fn set_permissions(
        &self,
        user_id: U,
        permissions: Permissions,
    ) -> Result<(), FunboyError> {
        let user_ctx = self.get_or_insert(user_id.clone()).await?;

        let mut current_permissions = user_ctx.permissions.lock().await;

        if current_permissions.is_owner() {
            return Err(PermissionError::CannotChangeOwnersRole.into());
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
        &self,
        user_id: U,
        permissions: &[Permission],
    ) -> Result<(), FunboyError> {
        if permissions.contains(&Permission::Owner) {
            return Err(PermissionError::CannotGrantOwnerPermission.into());
        }

        let user_ctx = self.get_or_insert(user_id.clone()).await?;

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
        &self,
        user_id: U,
        permissions: &[Permission],
    ) -> Result<(), FunboyError> {
        if permissions.contains(&Permission::Owner) {
            return Err(PermissionError::CannotRevokeOwnerPermission.into());
        }

        let user_ctx = self.get_or_insert(user_id.clone()).await?;
        let mut current_permissions = user_ctx.permissions.lock().await;

        if current_permissions.is_owner() {
            return Err(PermissionError::CannotRevokePermissionsFromOwner.into());
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

    pub async fn set_role(&self, user_id: U, role: Role) -> Result<(), FunboyError> {
        self.set_permissions(user_id, role.into()).await
    }

    pub async fn get_permissions(&self, user_id: U) -> Result<Permissions, FunboyError> {
        let user_ctx = self.get_or_insert(user_id.clone()).await?;
        let permissions = user_ctx.permissions.lock().await;
        Ok(permissions.clone())
    }
}
