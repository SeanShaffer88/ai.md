//! ai.md's document engine, free of any UI: the block model, DocxXML
//! reading and writing, the editing core with undo, the library on disk,
//! and Markdown / HTML import plus Markdown export, the prompt template
//! engine, and the bundled catalog.

pub mod catalog;
pub mod docxml;
pub mod editor;
pub mod emoji;
pub mod lang;
pub mod markdown;
pub mod model;
pub mod palette;
pub mod paste;
pub mod prompt;
pub mod store;
pub mod time;
