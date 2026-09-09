//! Safe Rust ownership wrapper for the canonical sterile C++ WarPigs engine.
//!
//! The wrapper owns the native handle and validates public parameters before
//! crossing the FFI boundary. The underlying engine is an in-memory simulator.

use std::fmt;
use std::ptr::NonNull;

#[repr(C)]
struct RawEngine {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn wp_engine_create(
        population_size: u64,
        max_population: u64,
        seed: u64,
        out_engine: *mut *mut RawEngine,
    ) -> u32;
    fn wp_engine_destroy(engine: *mut RawEngine);
    fn wp_engine_step(engine: *mut RawEngine, action: u32) -> u32;
    fn wp_engine_tick(engine: *const RawEngine) -> u64;
    fn wp_engine_population(engine: *const RawEngine) -> u64;
    fn wp_engine_configuration_code(
        engine: *const RawEngine,
        index: u64,
        out_code: *mut u8,
    ) -> u32;
    fn wp_engine_configuration_text(
        engine: *const RawEngine,
        index: u64,
        out: *mut u8,
        capacity: usize,
    ) -> u32;
    fn wp_engine_lifecycle(
        engine: *const RawEngine,
        index: u64,
        out_state: *mut u32,
    ) -> u32;
    fn wp_engine_abi_version() -> u32;
    fn wp_engine_configuration_count() -> u32;
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Error {
    InvalidArgument,
    Limit,
    Bounds,
    State,
    Buffer,
    Internal,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidArgument => "invalid argument",
            Self::Limit => "simulation limit exceeded",
            Self::Bounds => "index out of bounds",
            Self::State => "invalid lifecycle state transition",
            Self::Buffer => "output buffer too small",
            Self::Internal => "native engine internal error",
        })
    }
}

impl std::error::Error for Error {}

fn map_error(error: u32) -> Result<(), Error> {
    match error {
        0 => Ok(()),
        2 => Err(Error::InvalidArgument),
        3 => Err(Error::Limit),
        4 => Err(Error::Bounds),
        5 => Err(Error::State),
        6 => Err(Error::Buffer),
        1 | 255 => Err(Error::Internal),
        _ => Err(Error::Internal),
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Action {
    Prepare,
    Start,
    Quarantine,
    Terminate,
}

impl Action {
    fn raw(self) -> u32 {
        match self {
            Self::Prepare => 0,
            Self::Start => 1,
            Self::Quarantine => 2,
            Self::Terminate => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Lifecycle {
    Created,
    Ready,
    Running,
    Quarantined,
    Terminated,
}

impl TryFrom<u32> for Lifecycle {
    type Error = Error;

    fn try_from(state: u32) -> Result<Self, Self::Error> {
        match state {
            0 => Ok(Self::Created),
            1 => Ok(Self::Ready),
            2 => Ok(Self::Running),
            3 => Ok(Self::Quarantined),
            4 => Ok(Self::Terminated),
            _ => Err(Error::Internal),
        }
    }
}

pub struct Engine {
    raw: NonNull<RawEngine>,
}

impl Engine {
    pub const ABSOLUTE_MAX_POPULATION: u64 = 1_000_000;

    pub fn new(population_size: u64, max_population: u64, seed: u64) -> Result<Self, Error> {
        if max_population == 0 || max_population > Self::ABSOLUTE_MAX_POPULATION {
            return Err(Error::Limit);
        }
        if population_size > max_population {
            return Err(Error::Limit);
        }

        let mut raw = std::ptr::null_mut();
        // SAFETY: out_engine points to local writable storage. The native side
        // initializes one opaque handle on success and performs no callbacks.
        let error = unsafe { wp_engine_create(population_size, max_population, seed, &mut raw) };
        map_error(error)?;
        let raw = NonNull::new(raw).ok_or(Error::Internal)?;
        Ok(Self { raw })
    }

    pub fn abi_version() -> u32 {
        // SAFETY: pure constant query with no pointers or mutation.
        unsafe { wp_engine_abi_version() }
    }

    pub fn configuration_count() -> u32 {
        // SAFETY: pure constant query with no pointers or mutation.
        unsafe { wp_engine_configuration_count() }
    }

    pub fn population(&self) -> u64 {
        // SAFETY: self owns a valid native handle for its lifetime.
        unsafe { wp_engine_population(self.raw.as_ptr()) }
    }

    pub fn tick(&self) -> u64 {
        // SAFETY: self owns a valid native handle for its lifetime.
        unsafe { wp_engine_tick(self.raw.as_ptr()) }
    }

    pub fn step(&mut self, action: Action) -> Result<(), Error> {
        // SAFETY: raw is uniquely owned by this Engine and action comes from
        // the validated Rust enum.
        let error = unsafe { wp_engine_step(self.raw.as_ptr(), action.raw()) };
        map_error(error)
    }

    pub fn configuration(&self, index: u64) -> Result<String, Error> {
        let mut buffer = [0u8; 5];
        // SAFETY: caller-owned 5-byte buffer is valid for four digits + NUL.
        let error = unsafe {
            wp_engine_configuration_text(self.raw.as_ptr(), index, buffer.as_mut_ptr(), buffer.len())
        };
        map_error(error)?;
        let text = std::str::from_utf8(&buffer[..4]).map_err(|_| Error::Internal)?;
        Ok(text.to_owned())
    }

    pub fn configuration_code(&self, index: u64) -> Result<u8, Error> {
        let mut code = 0u8;
        // SAFETY: caller-owned output pointer is valid for one byte.
        let error = unsafe { wp_engine_configuration_code(self.raw.as_ptr(), index, &mut code) };
        map_error(error)?;
        Ok(code)
    }

    pub fn lifecycle(&self, index: u64) -> Result<Lifecycle, Error> {
        let mut raw = 0u32;
        // SAFETY: caller-owned output pointer is valid for one value.
        let error = unsafe { wp_engine_lifecycle(self.raw.as_ptr(), index, &mut raw) };
        map_error(error)?;
        Lifecycle::try_from(raw)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: raw is owned exclusively by self and released exactly once.
        unsafe { wp_engine_destroy(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Engine, Error, Lifecycle};

    #[test]
    fn abi_contract_is_explicit() {
        assert_eq!(Engine::abi_version(), 1);
        assert_eq!(Engine::configuration_count(), 81);
    }

    #[test]
    fn deterministic_configuration() {
        let a = Engine::new(64, 64, 42).unwrap();
        let b = Engine::new(64, 64, 42).unwrap();
        for index in 0..64 {
            assert_eq!(a.configuration(index).unwrap(), b.configuration(index).unwrap());
            assert_eq!(a.configuration_code(index).unwrap(), b.configuration_code(index).unwrap());
        }
    }

    #[test]
    fn lifecycle_and_population_are_bounded() {
        let mut engine = Engine::new(4, 4, 7).unwrap();
        assert_eq!(engine.population(), 4);
        assert_eq!(engine.tick(), 0);
        assert_eq!(engine.lifecycle(0).unwrap(), Lifecycle::Created);
        engine.step(Action::Prepare).unwrap();
        engine.step(Action::Start).unwrap();
        engine.step(Action::Quarantine).unwrap();
        engine.step(Action::Terminate).unwrap();
        assert_eq!(engine.lifecycle(0).unwrap(), Lifecycle::Terminated);
        assert_eq!(engine.tick(), 4);
        assert_eq!(engine.step(Action::Start), Err(Error::State));
        assert_eq!(engine.tick(), 4);
    }
}
