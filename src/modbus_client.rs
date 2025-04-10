// src/modbus_client.rs
use crate::error::AppError;
use crate::SystemCommand; // Import the command enum
use std::{net::SocketAddr, time::Duration};
use tokio::net::TcpStream; // For type hint in connect
use tokio::sync::broadcast;
use tokio::time::sleep;
use tokio_modbus::{
    client::*, // Import client components (connect, Context)
    prelude::Slave, // Import SlaveId and SlaveContext
};


// --- Modbus Register Definitions ---
const INVERTER_REG_MODE: u16 = 40231;
const INVERTER_REG_UNKNOWN1: u16 = 40191;
const INVERTER_REG_UNKNOWN2: u16 = 40187;

const INVERTER_OFF_MODE_VALUE: u16 = 3;
const INVERTER_OFF_UNKNOWN1_VALUE: u16 = 0;
const INVERTER_OFF_UNKNOWN2_VALUE: u16 = 0;

const SLAVE_ID: Slave = Slave(1); // Default Modbus Slave ID, adjust if needed

// --- Modbus Client Task ---
pub async fn task(
    addr_str: &str,
    mut rx: broadcast::Receiver<SystemCommand>,
) -> Result<(), AppError> {
    let socket_addr: SocketAddr = addr_str.parse().unwrap();
    log::info!("Starting Modbus TCP client task for {}", socket_addr);

    loop {
        // --- Connection Loop ---
        log::info!("Modbus Client ({}): Attempting to connect...", socket_addr);
        let stream = match TcpStream::connect(socket_addr).await {
             Ok(s) => {
                  log::info!("Modbus Client ({}): Connection established.", socket_addr);
                  s
             },
             Err(e) => {
                 log::error!("Modbus Client ({}): Connection failed: {}. Retrying in 5s.", socket_addr, e);
                 sleep(Duration::from_secs(5)).await;
                 continue; // Retry connection
             }
        };

        // Create Modbus context
        let mut ctx = tcp::attach_slave(stream, SLAVE_ID);


        // --- Command Processing Loop (while connected) ---
        loop {
            tokio::select! {
                // Biased select to prioritize receiving commands
                biased;

                // Wait for commands from the broadcast channel
                result = rx.recv() => {
                    match result {
                        Ok(command) => {
                            log::debug!("Modbus Client ({}): Received command: {:?}", socket_addr, command);
                            if command == SystemCommand::Off {
                                log::info!("Modbus Client ({}): Executing OFF sequence...", socket_addr);
                                // Perform writes
                                let writes = [
                                    (INVERTER_REG_MODE, INVERTER_OFF_MODE_VALUE),
                                    (INVERTER_REG_UNKNOWN1, INVERTER_OFF_UNKNOWN1_VALUE),
                                    (INVERTER_REG_UNKNOWN2, INVERTER_OFF_UNKNOWN2_VALUE),
                                ];
                                let mut success = true;
                                for (reg, val) in writes.iter() {
                                    log::debug!("Modbus Client ({}): Writing {} to register {}", socket_addr, val, reg);
                                    match ctx.write_single_register(*reg, *val).await {
                                        Ok(_) => {
                                             // Add small delay between writes if necessary for the inverter
                                             sleep(Duration::from_millis(50)).await;
                                        }
                                        Err(e) => {
                                            log::error!("Modbus Client ({}): Failed to write to register {}: {}", socket_addr, reg, e);
                                             success = false;
                                             break; // Break inner loop to reconnect
                                        }
                                    }
                                }
                                if success {
                                   log::info!("Modbus Client ({}): OFF sequence completed successfully.", socket_addr);
                                } else {
                                     log::warn!("Modbus Client ({}): OFF sequence failed or partially failed.", socket_addr);
                                     // If a write failed but didn't disconnect, we are still in the inner loop
                                }

                            } else if command == SystemCommand::On {
                                // "do nothing" as per requirement
                                log::info!("Modbus Client ({}): Received ON command (no action needed).", socket_addr);
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            log::warn!("Modbus Client ({}): Command channel closed. Exiting task.", socket_addr);
                            return Ok(()); // Graceful exit if channel closes
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                             log::warn!("Modbus Client ({}): Lagged behind on command channel by {} messages.", socket_addr, n);
                            // Continue receiving, processing the next available command
                        }
                    }
                }
                 // Add a check for connection health if needed, e.g., periodic read
                 _ = sleep(Duration::from_secs(30)) => { // Example: check every 30s
                    // log::trace!("Modbus Client ({}): Performing keep-alive read.", socket_addr);
                    // Try reading a harmless register to check the connection
                     match ctx.read_holding_registers(40070, 1).await { // Read register 40070
                          Ok(_) => { /* Connection seems okay */ }
                          Err(e) => {
                             log::error!("Modbus Client ({}): Keep-alive read failed: {}. Assuming disconnection.", socket_addr, e);
                             break; // Break inner loop to reconnect
                          }
                     }
                 }

            } // end tokio::select!
        } // end inner loop (while connected)

        // If we break the inner loop, it means connection was lost or a fatal error occurred.
        // The outer loop will handle the reconnection attempt after a delay.
        log::warn!("Modbus Client ({}): Connection lost or error occurred. Reconnecting...", socket_addr);
        // No explicit sleep here, outer loop handles it before retrying connect
    } // end outer loop (reconnection)
}