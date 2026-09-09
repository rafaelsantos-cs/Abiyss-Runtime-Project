//! Safe Rust ownership wrapper for the canonical sterile C++ WarPigs engine.
//!
//! The wrapper owns the native handle and validates public parameters before
//! crossing the FFI boundary. The underlying engine is an in-memory simulator.

use std::fmt;
use std::ptr::NonNull;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RawError {
    Ok = 0,
    Null = 1,
    InvalidArgument = 2,
    Limit = 3,
    Bounds = 4,
    State = 5,
    Buffer = 6,
    Internal = 255,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RawAction {
    Prepare = 0,
    Start = 1,
    Quarantine = 2,
    Terminate = 3,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RawLifecycle {
    Created = 0,
    Ready = 1,
    Running = 2,
    Quarantined = 3,
    Terminated = 4,
}

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
    ) -> RawError;
    fn wp_engine_destroy(engine: *mut RawEngine);
    fn wp_engine_step(engine: *mut RawEngine, action: RawAction) -> RawError;
    fn wp_engine_tick(engine: *const RawEngine) -> u64;
    fn wp_engine_population(engine: *const RawEngine) -> u64;
    fn wp_engine_configuration_code(
        engine: *const RawEngine,
        index: u64,
        out_code: *mut u8,
    ) -> RawError;
    fn wp_engine_configuration_text(
        engine: *const RawEngine,
        index: u64,
        out: *mut u8,
        capacity: usize,
    ) -> RawError;
    fn wp_engine_lifecycle(
        engine: *const RawEngine,
        index: u64,
        out_state: *mut RawLifecycle,
    ) -> RawError;
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

fn map_error(error: RawError) -> Result<(), Error> {
    match error {
        RawError::Ok => Ok(()),
        RawError::InvalidArgument => Err(Error::InvalidArgument),
        RawError::Limit => Err(Error::Limit),
        RawError::Bounds => Err(Error::Bounds),
        RawError::State => Err(Error::State),
        RawError::Buffer => Err(Error::Buffer),
        RawError::Internal | RawError::Null => Err(Error::Internal),
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Action {
    Prepare,
    Start,
    Quarantine,
    Terminate,
}

impl From<Action> for RawAction {
    fn from(action: Action) -> Self {
        match action {
            Action::Prepare => Self::Prepare,
            Action::Start => Self::Start,
            Action::Quarantine => Self::Quarantine,
            Action::Terminate => Self::Terminate,
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

impl TryFrom<RawLifecycle> for Lifecycle {
    type Error = Error;

    fn try_from(state: RawLifecycle) -> Result<Self, Self::Error> {
        Ok(match state {
            RawLifecycle::Created => Self::Created,
            RawLifecycle::Ready => Self::Ready,
            RawLifecycle::Running => Self::Running,
            RawLifecycle::Quarantined => Self::Quarantined,
            RawLifecycle::Terminated => Self::Terminated,
        })
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
        // SAFETY: `out_engine` is a valid writable pointer to local storage.
        // The C++ function initializes it on success and performs no Rust callbacks.
        let error = unsafe { wp_engine_create(population_size, max_population, seed, &mut raw) };
        map_error(error)?;
        let raw = NonNull::new(raw).ok_or(Error::Internal)?;
        Ok(Self { raw })
    }

    pub fn population(&self) -> u64 {
        // SAFETY: self.raw is owned and remains valid for the lifetime of self.
        unsafe { wp_engine_population(self.raw.as_ptr()) }
    }

    pub fn tick(&self) -> u64 {
        // SAFETY: self.raw is owned and remains valid for the lifetime of self.
        unsafe { wp_engine_tick(self.raw.as_ptr()) }
    }

    pub fn step(&mut self, action: Action) -> Result<(), Error> {
        // SAFETY: self.raw is uniquely owned by this Engine, and action is a
        // valid enum produced by the safe API.
        let error = unsafe { wp_engine_step(self.raw.as_ptr(), action.into()) };
        map_error(error)
    }

    pub fn configuration(&self, index: u64) -> Result<String, Error> {
        let mut buffer = [0u8; 5];
        // SAFETY: buffer is exactly large enough for four ASCII digits plus NUL.
        let error = unsafe {
            wp_engine_configuration_text(
                self.raw.as_ptr(),
                index,
                buffer.as_mut_ptr(),
                buffer.len(),
            )
        };
        map_error(error)?;
        let text = std::str::from_utf8(&buffer[..4]).map_err(|_| Error::Internal)?;
        Ok(text.to_owned())
    }

    pub fn configuration_code(&self, index: u64) -> Result<u8, Error> {
        let mut code = 0u8;
        // SAFETY: output pointer is valid for one byte.
        let error = unsafe {
            wp_engine_configuration_code(self.raw.as_ptr(), index, &mut code)
        };
        map_error(error)?;
        Ok(code)
    }

    pub fn lifecycle(&self, index: u64) -> Result<Lifecycle, Error> {
        let mut raw = RawLifecycle::Created;
        // SAFETY: output pointer is valid for the enum value.
        let error = unsafe { wp_engine_lifecycle(self.raw.as_ptr(), index, &mut raw) };
        map_error(error)?;
        Lifecycle::try_from(raw)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: raw is owned exclusively by self and is destroyed exactly once.
        unsafe { wp_engine_destroy(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Engine, Error, Lifecycle};

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
