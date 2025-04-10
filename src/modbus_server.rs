// src/modbus_server.rs
use crate::{
    data::BmsData, // Import specific register constants
    error::AppError,
};
use std::{
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, RwLock},
};
use tokio::net::TcpListener; // Use tokio::net::TcpListener
use tokio_modbus::{
    prelude::*, // Includes ExceptionCode, Request, Response etc.
    server::tcp::{Server, accept_tcp_connection},
};

// --- Custom Modbus Service ---
// Service struct remains the same
#[derive(Debug, Clone)] // Added Clone trait, needed for the service factory pattern
struct BmsModbusService {
    bms_data: Arc<RwLock<Option<BmsData>>>,
}

// Implement Service trait
// Using ExceptionCode as the error type as per tokio-modbus 0.9.x and user code
impl tokio_modbus::server::Service for BmsModbusService {
    type Request = Request<'static>; // Use static lifetime as in user code
    type Response = Response;
    type Exception = ExceptionCode; // Use ExceptionCode
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Exception>> + Send>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        // Clone Arc for use in the async block
        let data_lock = Arc::clone(&self.bms_data);

        Box::pin(async move {
            log::debug!("Received Modbus request: {:?}", req);

            match req {
                // --- Handle Read Holding Registers (0x03) ---
                Request::ReadHoldingRegisters(addr, cnt) => {
                    // Acquire read lock (no changes needed here for reads)
                    let data_guard = data_lock.read().map_err(|_| {
                        log::error!("ReadHoldingRegisters: Failed to acquire read lock (poisoned)");
                        ExceptionCode::ServerDeviceFailure
                    })?;

                    let maybe_data = &*data_guard;
                    match maybe_data {
                        Some(data) => {
                            let mut registers = Vec::with_capacity(cnt as usize);
                            for i in 0..cnt {
                                let current_addr = addr + i;
                                // Use get_register, defaulting to 0 if None (or address invalid)
                                // get_register now handles the 0xFF default for REG_BMS_INFO internally
                                let value = data.get_register(current_addr).unwrap_or(0);
                                registers.push(value);
                            }
                            log::trace!(
                                "Responding to ReadHoldingRegisters({}..{}) with: {:?}",
                                addr,
                                addr + cnt - 1,
                                registers
                            );
                            Ok(Response::ReadHoldingRegisters(registers))
                        }
                        None => {
                            // No BMS data object available yet (task hasn't run or failed)
                            // Decide how to respond: Zeros or specific error
                            log::warn!(
                                "ReadHoldingRegisters: No BmsData object available yet. Returning zeros."
                            );
                            let registers = vec![0; cnt as usize];
                            Ok(Response::ReadHoldingRegisters(registers))
                            // Or: Err(ExceptionCode::ServerDeviceFailure) ?
                        }
                    }
                }

                // --- Handle Read Input Registers (0x04) ---
                Request::ReadInputRegisters(addr, cnt) => {
                    // Logic is identical to ReadHoldingRegisters in this example
                    let data_guard = data_lock.read().map_err(|_| {
                        log::error!("ReadInputRegisters: Failed to acquire read lock (poisoned)");
                        ExceptionCode::ServerDeviceFailure
                    })?;

                    let maybe_data = &*data_guard;
                    match maybe_data {
                        Some(data) => {
                            let mut registers = Vec::with_capacity(cnt as usize);
                            for i in 0..cnt {
                                let current_addr = addr + i;
                                let value = data.get_register(current_addr).unwrap_or(0);
                                registers.push(value);
                            }
                            log::trace!(
                                "Responding to ReadInputRegisters({}..{}) with: {:?}",
                                addr,
                                addr + cnt - 1,
                                registers
                            );
                            Ok(Response::ReadInputRegisters(registers))
                        }
                        None => {
                            log::warn!(
                                "ReadInputRegisters: No BmsData object available yet. Returning zeros."
                            );
                            let registers = vec![0; cnt as usize];
                            Ok(Response::ReadInputRegisters(registers))
                        }
                    }
                }

                // --- Handle Write Single Register (0x06) ---
                Request::WriteSingleRegister(addr, value) => {
                    // Acquire write lock - needed to potentially modify data
                    let mut data_guard = data_lock.write().map_err(|_| {
                        log::error!("WriteSingleRegister: Failed to acquire write lock (poisoned)");
                        ExceptionCode::ServerDeviceFailure
                    })?;

                    // Get mutable reference, initializing if None
                    let data_ref = data_guard.get_or_insert_with(BmsData::default);

                    // Use the new set_register method which handles validation and updates
                    match data_ref.set_register(addr, value) {
                        Ok(()) => {
                            // Echo the request back on success, as per Modbus standard
                            Ok(Response::WriteSingleRegister(addr, value))
                        }
                        Err(exception_code) => {
                            // set_register returned an error (e.g., IllegalDataValue, IllegalFunction)
                            Err(exception_code)
                        }
                    }
                }

                // --- Handle Write Multiple Registers (0x10) ---
                // Still deny for now, but could be implemented similarly to WriteSingleRegister
                Request::WriteMultipleRegisters(addr, ref values) => {
                    let mut data_guard = data_lock
                        .write()
                        .map_err(|_| ExceptionCode::ServerDeviceFailure)?;
                    let data_ref = data_guard.get_or_insert_with(BmsData::default);
                    for (i, value) in values.iter().enumerate() {
                        let current_addr = addr + i as u16;
                        if let Err(e) = data_ref.set_register(current_addr, *value) {
                            // Decide on error handling: stop immediately or continue?
                            // Modbus standard often expects an error on the first failure.
                            log::error!(
                                "Error writing multiple registers at offset {}: {:?}",
                                i,
                                e
                            );
                            return Err(e); // Return the specific error
                        }
                    }
                    Ok(Response::WriteMultipleRegisters(addr, values.len() as u16))
                    // Err(ExceptionCode::IllegalFunction)
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
// Using the server setup structure provided in the user's code snippet
pub async fn task(
    addr_str: &str,
    bms_data: Arc<RwLock<Option<BmsData>>>,
) -> Result<(), AppError> {
    let socket_addr: SocketAddr = addr_str.parse().unwrap();
    log::info!("Starting Modbus TCP server on {}", socket_addr);
    let listener = TcpListener::bind(socket_addr).await?; // Use await and ?
    let server = Server::new(listener);

    // Factory closure to create a new service instance for each connection.
    // Clones the Arc<RwLock<...>> so each service instance shares the same data.
    let new_service = move |_socket_addr: SocketAddr| {
        // This closure is called by accept_tcp_connection for each new client.
        // It needs to return a Result<Option<Service>, io::Error>
        // The Option is Some if the connection is accepted, None otherwise.
        Ok(Some(BmsModbusService {
            // Clone the Arc here, so the new service instance gets a pointer to the shared data
            bms_data: Arc::clone(&bms_data),
        }))
    };

    // Wrap the factory closure in Arc for the on_connected handler
    let new_service_arc = Arc::new(new_service);

    // Handler for new connections
    let on_connected = {
        move |stream, socket_addr| {
            // Clone the Arc containing the factory closure for the async block
            let service_factory = Arc::clone(&new_service_arc);
            async move {
                log::info!("New Modbus client connected: {}", socket_addr);
                // Pass the stream, address, and the factory closure to accept_tcp_connection
                // The factory closure (*service_factory) will be called to create the service instance.
                accept_tcp_connection(stream, socket_addr, move |addr| (*service_factory)(addr))
            }
        }
    };

    // Handler for processing errors within a connection
    let on_process_error = move |err| {
        log::error!("Modbus connection error: {}", err);
    };

    // Start the server loop
    // server.serve requires references to the handlers
    if let Err(e) = server.serve(&on_connected, on_process_error).await {
        log::error!("Modbus server failed: {}", e);
        return Err(AppError::ModbusIo(e)); // Map io::Error to AppError::ModbusIo
    }

    // This part is likely unreachable if serve runs indefinitely
    log::warn!("Modbus TCP server on {} has stopped.", socket_addr);
    Ok(())
}
