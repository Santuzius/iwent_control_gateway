// src/error.rs
use thiserror::Error;
use std::io;
use tokio::sync::{broadcast, mpsc}; // For channel send errors

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

    // --- New Errors ---
    #[error("GPIO error: {0}")]
    Gpio(#[from] rppal::gpio::Error),

    #[error("GPIO unavailable on this platform")]
    GpioUnavailable, // For non-Pi builds

    #[error("Broadcast channel send error: {0}")]
    BroadcastSendError(#[from] broadcast::error::SendError<crate::SystemCommand>), // Use specific command type

    #[error("MPSC channel send error (GPIO state): {0}")]
    MpscSendErrorGpio(#[from] mpsc::error::SendError<bool>),

    #[error("Modbus client connection error: {0}")]
    ModbusClientConnection(io::Error), // Specific error for client connection issues

    #[error("Modbus client operation error: {0}")]
    ModbusClientOperation(#[from] tokio_modbus::prelude::ExceptionCode),

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