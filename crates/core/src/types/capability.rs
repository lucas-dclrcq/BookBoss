use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub type Capabilities = HashSet<Capability>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    Admin,
    ApproveImports,
    ConvertBook,
    DeleteBook,
    EditBook,
    OpdsAccess,
    SuperAdmin,
}

impl Capability {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "Admin",
            Self::ApproveImports => "ApproveImports",
            Self::ConvertBook => "ConvertBook",
            Self::DeleteBook => "DeleteBook",
            Self::EditBook => "EditBook",
            Self::OpdsAccess => "OpdsAccess",
            Self::SuperAdmin => "SuperAdmin",
        }
    }

    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Admin => "Admin",
            Self::ApproveImports => "Approve Imports",
            Self::ConvertBook => "Convert Books",
            Self::DeleteBook => "Delete Books",
            Self::EditBook => "Edit Books",
            Self::OpdsAccess => "OPDS Access",
            Self::SuperAdmin => "Super Admin",
        }
    }

    /// All granular capabilities that can be individually granted to a User
    /// role. Excludes Admin and `SuperAdmin` which are role-level
    /// designations.
    #[must_use]
    pub fn user_grantable() -> &'static [Self] {
        &[Self::ApproveImports, Self::ConvertBook, Self::DeleteBook, Self::EditBook, Self::OpdsAccess]
    }
}
