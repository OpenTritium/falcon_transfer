#![feature(ip)]
#![feature(duration_constructors)]
#![feature(let_chains)]
#![feature(slice_index_methods)]
#![feature(slice_pattern)]
#![feature(slice_as_array)]

use std::future::pending;

pub mod env;
pub mod hot_file;
pub mod link;
pub mod utils;
