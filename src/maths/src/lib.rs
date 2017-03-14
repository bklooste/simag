//! Support mathematical methods library for the simAG framework
#![feature(test)]

extern crate float_cmp;
extern crate ndarray;
extern crate test;
extern crate itertools;
extern crate rand;

mod model;
mod sampling;
pub mod dists;

const FLOAT_EQ_ULPS: i64 = 2;

/// Probability type
pub type P = f64;

pub use model::DiscreteModel;
