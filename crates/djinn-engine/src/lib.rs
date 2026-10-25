//! DJINN audio engine: decode, mix, and fan out to output devices.
//!
//! This crate must stay free of web, database, and async runtime
//! dependencies. The server talks to it through command and event channels.
