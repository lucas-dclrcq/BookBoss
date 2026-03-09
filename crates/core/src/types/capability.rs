use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub type Capabilities = HashSet<Capability>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    Admin,
    ApproveImports,
    ConvertBook,
    DeleteBook,
    EditBook,
    SuperAdmin,
}

impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "Admin",
            Self::ApproveImports => "ApproveImports",
            Self::ConvertBook => "ConvertBook",
            Self::DeleteBook => "DeleteBook",
            Self::EditBook => "EditBook",
            Self::SuperAdmin => "SuperAdmin",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Admin => "Admin",
            Self::ApproveImports => "Approve Imports",
            Self::ConvertBook => "Convert Books",
            Self::DeleteBook => "Delete Books",
            Self::EditBook => "Edit Books",
            Self::SuperAdmin => "Super Admin",
        }
    }

    /// All granular capabilities that can be individually granted to a User
    /// role. Excludes Admin and SuperAdmin which are role-level
    /// designations.
    pub fn user_grantable() -> &'static [Capability] {
        &[Self::ApproveImports, Self::ConvertBook, Self::DeleteBook, Self::EditBook]
    }
}
