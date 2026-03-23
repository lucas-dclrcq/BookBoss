mod parse;
mod write;

pub use parse::{CoverInfo, extract_cover_href, extract_cover_info, extract_metadata, parse_sidecar};
pub(crate) use write::write_metadata_xml;
pub use write::write_sidecar;

#[cfg(test)]
mod regression_tests;
