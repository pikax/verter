//! Transport: portable tsgo binary discovery, spawn-arg construction, the
//! STANDALONE OS duplex channel, and the `--api` ATTACH pipe connector.
//!
//! The STANDALONE duplex channel ([`pipe`]) uses the spawned engine's
//! stdin/stdout pipes on EVERY platform (`tsgo --api` speaks the MessagePack
//! tuple protocol over stdio; the Windows-named-pipe dance the shipped JS sync
//! client performs is a Node synchronous-fd workaround we do not need with async
//! tokio I/O). The separate ATTACH connector ([`pipe_attach`]) DOES connect to a
//! server-minted named pipe / UDS — the pipe the `tsgo --lsp` server mints for
//! `custom/initializeAPISession`.

pub mod pipe;
pub mod pipe_attach;
pub mod spawn;
