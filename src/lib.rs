pub mod input;
pub mod markdown;
pub mod output;
pub mod render;
pub mod world;

pub type AppResult<T> = Result<T, Box<dyn std::error::Error>>;
