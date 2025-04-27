#![feature(ip)]
#![feature(duration_constructors)]
#![feature(let_chains)]
#![feature(slice_index_methods)]
#![feature(slice_pattern)]
#![feature(slice_as_array)]
#![feature(likely_unlikely)]
#![feature(portable_simd)]
#![feature(iter_next_chunk)]
#![feature(ip_from)]
#![feature(async_iterator)]
#![feature(once_cell_get_mut)]
#![feature(once_cell_try)]

pub mod config;
// pub mod event_handler;
pub mod hot_file;
pub mod addr;
pub mod inbound;
// pub mod outbound;
pub mod link;
// pub mod session;
