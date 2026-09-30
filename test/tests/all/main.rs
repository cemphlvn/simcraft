//! All integration tests in one program: the engine is linked once, not once per file.
//! Add a file here as a module (`mod name;`), not as a new file in `test/tests/`.

mod engine;
mod kernel;
mod physics;
mod properties;
mod render;
mod scenarios;
mod work;
