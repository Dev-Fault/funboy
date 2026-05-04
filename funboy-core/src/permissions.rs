use std::collections::HashSet;

use clap::ValueEnum;
use strum_macros::EnumString;

use crate::FunboyError;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, ValueEnum, strum_macros::Display, EnumString)]
pub enum Permission {
    Owner,
    Exec,
    File,
    Create,
    Update,
    Generate,
    Ollama,
    Grant,
    Revoke,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::Owner => "Owner",
            Permission::Exec => "Exec",
            Permission::File => "File",
            Permission::Create => "Create",
            Permission::Update => "Update",
            Permission::Generate => "Generate",
            Permission::Ollama => "Ollama",
            Permission::Grant => "Grant",
            Permission::Revoke => "Revoke",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, ValueEnum, strum_macros::Display, EnumString)]
pub enum Role {
    Admin,
    Member,
    Guest,
    Observer,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "Admin",
            Role::Member => "Member",
            Role::Guest => "Guest",
            Role::Observer => "Observer",
        }
    }
}

impl Into<Permissions> for Role {
    fn into(self) -> Permissions {
        match self {
            Role::Admin => Permissions::admin(),
            Role::Member => Permissions::member(),
            Role::Guest => Permissions::guest(),
            Role::Observer => Permissions::observer(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Permissions(pub HashSet<Permission>);

impl std::fmt::Display for Permissions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = vec![];
        for permission in self.0.iter() {
            out.push(permission.to_string());
        }
        write!(f, "{}", out.join(", "))
    }
}

impl From<&[Permission]> for Permissions {
    fn from(value: &[Permission]) -> Self {
        Permissions(HashSet::from_iter(value.iter().cloned()))
    }
}

impl From<HashSet<Permission>> for Permissions {
    fn from(value: HashSet<Permission>) -> Self {
        Self(value)
    }
}

impl Default for Permissions {
    fn default() -> Self {
        Permissions::guest()
    }
}

impl Permissions {
    pub fn owner() -> Self {
        Permissions(HashSet::from([
            Permission::Owner,
            Permission::Exec,
            Permission::File,
            Permission::Create,
            Permission::Update,
            Permission::Generate,
            Permission::Ollama,
            Permission::Grant,
            Permission::Revoke,
        ]))
    }

    pub fn admin() -> Self {
        Permissions(HashSet::from([
            Permission::File,
            Permission::Create,
            Permission::Update,
            Permission::Generate,
            Permission::Ollama,
            Permission::Grant,
            Permission::Revoke,
        ]))
    }

    pub fn member() -> Self {
        Permissions(HashSet::from([
            Permission::Create,
            Permission::Update,
            Permission::Generate,
            Permission::Ollama,
        ]))
    }

    pub fn guest() -> Self {
        Permissions(HashSet::from([Permission::Generate, Permission::Ollama]))
    }

    pub fn observer() -> Self {
        Permissions(HashSet::new())
    }

    pub fn can_use_files(&self) -> bool {
        self.0.contains(&Permission::File)
    }

    pub fn can_generate(&self) -> bool {
        self.0.contains(&Permission::Generate)
    }

    pub fn can_create(&self) -> bool {
        self.0.contains(&Permission::Create)
    }

    pub fn can_update(&self) -> bool {
        self.0.contains(&Permission::Update)
    }

    pub fn can_use_ollama(&self) -> bool {
        self.0.contains(&Permission::Ollama)
    }

    pub fn can_grant(&self) -> bool {
        self.0.contains(&Permission::Grant)
    }

    pub fn can_revoke(&self) -> bool {
        self.0.contains(&Permission::Revoke)
    }

    pub fn can_exec(&self) -> bool {
        self.0.contains(&Permission::Exec)
    }

    pub fn has_permission(&self, permission: Permission) -> bool {
        self.0.contains(&permission)
    }

    pub fn is_owner(&self) -> bool {
        self.0.contains(&Permission::Owner)
    }

    pub fn get_lacking(&self, required_permissions: &[Permission]) -> Permissions {
        Permissions::from(
            required_permissions
                .iter()
                .filter(|p| !self.0.contains(p))
                .map(|p| p.to_owned())
                .collect::<HashSet<Permission>>(),
        )
    }
}

#[derive(Debug, Clone)]
pub enum PermissionError {
    CannotGrantOwnerPermission,
    CannotRevokeOwnerPermission,
    CannotRevokePermissionsFromOwner,
    CannotChangeOwnersRole,
}

impl Into<FunboyError> for PermissionError {
    fn into(self) -> FunboyError {
        FunboyError::Permission(self)
    }
}

impl ToString for PermissionError {
    fn to_string(&self) -> String {
        match self {
            PermissionError::CannotGrantOwnerPermission => "owner permission cannot be granted",
            PermissionError::CannotChangeOwnersRole => "Owners role cannot be changed",
            PermissionError::CannotRevokeOwnerPermission => "owner permission cannot be revoked",
            PermissionError::CannotRevokePermissionsFromOwner => {
                "permissions cannot be revoked from owner"
            }
        }
        .to_owned()
    }
}
