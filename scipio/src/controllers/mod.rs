// Unless explicitly stated otherwise all files in this repository are licensed under the
// MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2020 Datadog, Inc.
//! scipio::controllers provide helpful constructs to automatically control the
//! shares, and in consequence the proportion of resources, that a particular process
//! uses.
//!
//! It implements data structures with embedded controllers derived from work in
//! control theory like the [`PID controller`].
//!
//! [`PID controller`]: https://en.wikipedia.org/wiki/PID_controller
mod deadline_queue;
pub use self::deadline_queue::*;