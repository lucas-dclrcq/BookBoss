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
}
