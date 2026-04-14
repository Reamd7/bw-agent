#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("placeholder")]
    Placeholder,
}

pub type Result<T> = std::result::Result<T, Error>;
