mod binary;
mod text;
mod tree;
mod xml;

pub use binary::{BinaryWriteOptions, read_binary, write_binary};
pub use text::{format_f32, format_f64, parse_bool, parse_f32, parse_f64, parse_i32, parse_i64};
pub use tree::{Tree, TreeInstance};
pub use xml::{read_xml, write_xml};

pub const BINARY_MAGIC: &[u8; 8] = b"<roblox!";

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("unrecognized format")]
    UnrecognizedFormat,
    #[error("the file header is corrupted")]
    CorruptHeader,
    #[error("unrecognized version {0}")]
    UnsupportedVersion(u16),
    #[error("unexpected end of file")]
    UnexpectedEof,
    #[error("read offset is out of bounds while reading {0} bytes")]
    OutOfBounds(usize),
    #[error("malformed data ({0} != {1})")]
    Decompress(usize, usize),
    #[error("type index out of bounds: {0}")]
    TypeIndex(u32),
    #[error("duplicate type index: {0}")]
    DuplicateType(u32),
    #[error("invalid id {0}")]
    InvalidId(i64),
    #[error("duplicate id {0}")]
    DuplicateId(i64),
    #[error("unrecognized object format {0}")]
    ObjectFormat(u8),
    #[error("unrecognized parent link format {0}")]
    ParentFormat(u8),
    #[error("unexpected property format {0} (expected {1})")]
    PropertyFormat(u8, u8),
    #[error("unknown property format {0} for property {1}")]
    UnknownPropertyFormat(u8, String),
    #[error("property {0} has a type the binary format can't store")]
    UnsupportedBinaryType(String),
    #[error("xml: {0}")]
    Xml(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub fn is_binary(data: &[u8]) -> bool {
    data.len() >= 8 && &data[..8] == BINARY_MAGIC
}

pub fn read(data: &[u8]) -> Result<Tree, FormatError> {
    if data.len() < 8 {
        return Err(FormatError::UnrecognizedFormat);
    }
    if is_binary(data) {
        read_binary(data)
    } else {
        read_xml(data)
    }
}
