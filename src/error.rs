// src/error.rs
use thiserror::Error;
use std::io;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("CAN socket error: {0}")]
    CanSocket(#[from] io::Error), // Covers socketcan I/O errors

    #[error("Modbus IO error: {0}")]
    ModbusIo(io::Error), // Separate Modbus IO errors if needed

    #[error("Invalid CAN data length for ID {can_id:#X}: expected {expected}, got {actual}")]
    InvalidCanDataLength {
        can_id: u32,
        expected: usize,
        actual: usize,
    },

    #[error("Unsupported CAN ID: {0:#X}")]
    UnsupportedCanId(u32),

    #[error("Lock is poisoned")]
    LockPoisoned,

    #[error("Task join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),

    // Add other specific error types as needed
    #[error("Unknown error")]
    _Unknown,
}

// Implement conversion from PoisonError if needed, simplified here
impl<T> From<std::sync::PoisonError<T>> for AppError {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        AppError::LockPoisoned
    }
}

// Explicit conversion for Modbus IO errors if tokio-modbus doesn't use std::io::Error directly in its public API often
impl From<tokio_modbus::prelude::ExceptionCode> for AppError {
   fn from(e: tokio_modbus::prelude::ExceptionCode) -> Self {
       // You might want to inspect the inner error type here for more specific mapping
       AppError::ModbusIo(io::Error::new(io::ErrorKind::Other, format!("Modbus Slave IO Error: {}", e)))
   }
}