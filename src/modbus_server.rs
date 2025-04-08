// src/modbus_server.rs
use crate::{
    data::BmsData,
    error::AppError,
};
use std::{
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, RwLock},
};
use tokio::net::TcpListener;
use tokio_modbus::{
    prelude::*,
    server::tcp::{accept_tcp_connection, Server},
};

// --- Custom Modbus Service ---
#[derive(Debug)] // Added Debug trait
struct BmsModbusService {
    bms_data: Arc<RwLock<Option<BmsData>>>,
}

impl tokio_modbus::server::Service for BmsModbusService {
    type Request = Request<'static>;
    type Response = Response;
    type Exception = ExceptionCode;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Exception>> + Send>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        // Clone Arc for use in the async block
        let data_lock = Arc::clone(&self.bms_data);

        Box::pin(async move {
            log::debug!("Received Modbus request: {:?}", req);

            // Acquire read lock
            let data_guard = match data_lock.read() {
                 Ok(guard) => guard,
                 Err(_) => {
                     log::error!("Modbus service: Failed to acquire read lock (poisoned)");
                     // Respond with a server device failure or similar Modbus exception
                     return Err(ExceptionCode::ServerDeviceFailure);
                 }
            };

            // Access the Option<BmsData>
            let maybe_data = &*data_guard;

            match req {
                // Handle Read Holding Registers (0x03)
                Request::ReadHoldingRegisters(addr, cnt) => {
                    match maybe_data {
                        Some(data) => {
                            let mut registers = Vec::with_capacity(cnt as usize);
                            // Iterate over the requested register range
                            for i in 0..cnt {
                                let current_addr = addr + i;
                                // Get the register value from BmsData, default to 0 if None or out of range
                                let value = data.get_register(current_addr).unwrap_or(0);
                                registers.push(value);
                            }
                             log::trace!("Responding to ReadHoldingRegisters({}..{}) with: {:?}", addr, addr+cnt-1, registers);
                            Ok(Response::ReadHoldingRegisters(registers))
                        }
                        None => {
                            // No data available yet, return empty registers or an error
                             // Option 1: Return zeros
                            log::warn!("ReadHoldingRegisters: No BMS data available yet. Returning zeros.");
                            let registers = vec![0; cnt as usize];
                            Ok(Response::ReadHoldingRegisters(registers))
                             // Option 2: Return test values
                            // log::warn!("ReadHoldingRegisters: No BMS data available yet. Returning testvalues.");
                            // let registers = vec![38631, 38832, 19, 21, 82, 5, 7453, 0, 0, 0, 0, 0];
                            // Ok(Response::ReadHoldingRegisters(registers))
                             // Option 3: Return Modbus Exception (e.g., SlaveDeviceFailure)
                             // Err(SlaveIoError::SlaveDeviceFailure)
                        }
                    }
                }

                // Handle Read Input Registers (0x04) - Often used for read-only data
                Request::ReadInputRegisters(addr, cnt) => {
                   // Same logic as ReadHoldingRegisters for this example
                   match maybe_data {
                        Some(data) => {
                            let mut registers = Vec::with_capacity(cnt as usize);
                            for i in 0..cnt {
                                let current_addr = addr + i;
                                let value = data.get_register(current_addr).unwrap_or(0);
                                registers.push(value);
                            }
                            log::trace!("Responding to ReadInputRegisters({}..{}) with: {:?}", addr, addr+cnt-1, registers);
                            Ok(Response::ReadInputRegisters(registers))
                        }
                        None => {
                            log::warn!("ReadInputRegisters: No BMS data available yet. Returning zeros.");
                            let registers = vec![0; cnt as usize];
                            Ok(Response::ReadInputRegisters(registers))
                            // Err(SlaveIoError::SlaveDeviceFailure)
                        }
                    }
                }

                 // --- Add handlers for other function codes if needed ---
                 // Example: WriteSingleRegister (0x06) - Respond with IllegalFunction if not supported
                 Request::WriteSingleRegister(addr, _value) => {
                     log::warn!("WriteSingleRegister to address {} denied (read-only)", addr);
                     Err(ExceptionCode::IllegalFunction) // Or IllegalDataAddress if appropriate
                 }

                 // Example: WriteMultipleRegisters (0x10)
                 Request::WriteMultipleRegisters(addr, _) => {
                     log::warn!("WriteMultipleRegisters to address {} denied (read-only)", addr);
                     Err(ExceptionCode::IllegalFunction)
                 }

                 // Default handler for unsupported function codes
                 _ => {
                     log::warn!("Unsupported Modbus function code received: {:?}", req);
                     Err(ExceptionCode::IllegalFunction)
                 }
            }
        })
    }
}

// --- Modbus Server Task ---
pub async fn server(addr_str: &str, bms_data: Arc<RwLock<Option<BmsData>>>) -> Result<(), AppError> {
    
    let socket_addr: SocketAddr = addr_str.parse().unwrap();
    log::info!("Starting Modbus TCP server on {}", socket_addr);
    let listener = TcpListener::bind(socket_addr).await?;
    let server = Server::new(listener);

    // Create the new_service closure which clones bms_data
    let new_service = {
        let bms_data = Arc::clone(&bms_data);
        move |_socket_addr: SocketAddr| {
            Ok(Some(BmsModbusService {
                bms_data: Arc::clone(&bms_data),
            }))
        }
    };

    // Wrap new_service in an Arc so it can be used multiple times
    let new_service = Arc::new(new_service);

    // Define on_connected so that it clones new_service for each new connection
    let on_connected = {
        let new_service = Arc::clone(&new_service);
        move |stream, socket_addr| {
            // Clone new_service again here so it's available inside the async block
            let new_service = Arc::clone(&new_service);
            async move {
                log::info!("Accepting new connection on {}", socket_addr);
                // Pass a closure that calls new_service.
                // Since new_service is an Arc, each call gets a fresh reference.
                accept_tcp_connection(stream, socket_addr, |addr| (*new_service)(addr))
            }
        }
    };

    let on_process_error = move |err| {
        log::error!("Modbus server error on {}: {}", socket_addr, err);
    };

    server.serve(&on_connected, on_process_error).await?;

    Ok(())
}