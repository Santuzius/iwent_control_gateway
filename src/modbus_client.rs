// src/modbus_client.rs
use crate::error::AppError;
use crate::SystemCommand;
use std::{net::SocketAddr, time::Duration};
use tokio::net::TcpStream;
// Behalten Sie crossbeam für die Signatur bei, wenn nötig
use tokio::time::sleep;
use tokio_modbus::{
    client::*,
    prelude::{Client, Slave},
};

// --- Modbus Register Definitions (unverändert) ---
const INVERTER_REG_MODE: u16 = 40231;
const INVERTER_REG_UNKNOWN1: u16 = 40191;
const INVERTER_REG_UNKNOWN2: u16 = 40187;

const INVERTER_OFF_MODE_VALUE: u16 = 3;
const INVERTER_OFF_UNKNOWN1_VALUE: u16 = 0;
const INVERTER_OFF_UNKNOWN2_VALUE: u16 = 0;

const SLAVE_ID: Slave = Slave(1);

// --- Helper Function for Inverter OFF Sequence (unverändert) ---
async fn execute_inverter_off_sequence<C>(
    ctx: &mut C,
    socket_addr: &SocketAddr,
) -> Result<(), tokio_modbus::Error>
where
    C: Client + Unpin + tokio_modbus::prelude::Writer,
{
    log::info!("Modbus Client ({}): Executing OFF sequence...", socket_addr);
    let writes = [
        (INVERTER_REG_MODE, INVERTER_OFF_MODE_VALUE),
        (INVERTER_REG_UNKNOWN1, INVERTER_OFF_UNKNOWN1_VALUE),
        (INVERTER_REG_UNKNOWN2, INVERTER_OFF_UNKNOWN2_VALUE),
    ];

    for (reg, val) in writes.iter() {
        log::debug!(
            "Modbus Client ({}): Writing {} to register {}",
            socket_addr,
            val,
            reg
        );
        ctx.write_single_register(*reg, *val).await?;
        sleep(Duration::from_millis(50)).await;
    }

    log::info!(
        "Modbus Client ({}): OFF sequence completed successfully.",
        socket_addr
    );
    Ok(())
}

// --- Modbus Client Task ---
pub async fn task(
    addr_str: &str,
    // Behalten Sie die crossbeam-Typen bei, wenn diese von außen kommen
    error_rx: crossbeam_channel::Receiver<()>,
    output_rx: crossbeam_channel::Receiver<SystemCommand>,
) -> Result<(), AppError> {
    let socket_addr: SocketAddr = addr_str.parse().map_err(|e| {
        log::error!("Invalid socket address format '{}': {}", addr_str, e);
    }).unwrap();

    log::info!("Starting Modbus TCP client task for {}", socket_addr);

    loop {
        // --- Connection Loop (unverändert) ---
        log::info!("Modbus Client ({}): Attempting to connect...", socket_addr);
        let stream = match TcpStream::connect(socket_addr).await {
            Ok(s) => {
                log::info!("Modbus Client ({}): Connection established.", socket_addr);
                s
            }
            Err(e) => {
                log::error!(
                    "Modbus Client ({}): Connection failed: {}. Retrying in 5s.",
                    socket_addr,
                    e
                );
                sleep(Duration::from_secs(5)).await;
                continue; // Retry connection
            }
        };

        // Create Modbus context (unverändert)
        let mut ctx = tcp::attach_slave(stream, SLAVE_ID);

        // --- Command Processing Loop (while connected) ---
        'inner: loop {
            // Clone receivers for use in spawn_blocking.
            // crossbeam Receivers are Clone.
            let output_rx_c = output_rx.clone();
            let error_rx_c = error_rx.clone();

            tokio::select! {
                biased; // Prioritize receiving commands/errors over keep-alive

                // Wrap the blocking crossbeam recv in spawn_blocking
                result = tokio::task::spawn_blocking(move || output_rx_c.recv()) => {
                    // spawn_blocking returns a Result<T, JoinError> where T is the result of the closure
                    match result {
                        // Closure finished successfully, check its Result
                        Ok(Ok(command)) => {
                            log::debug!("Modbus Client ({}): Received command: {:?}", socket_addr, command);
                            match command {
                                SystemCommand::Off => {
                                    match execute_inverter_off_sequence(&mut ctx, &socket_addr).await {
                                        Ok(_) => { /* Success logged in helper */ }
                                        Err(e) => {
                                            log::error!("Modbus Client ({}): OFF sequence failed during command execution: {}", socket_addr, e);
                                            break 'inner; // Reconnect on failure
                                        }
                                    }
                                }
                                SystemCommand::On => {
                                    log::info!("Modbus Client ({}): Received ON command (no action needed).", socket_addr);
                                }
                                SystemCommand::Quit => {
                                    log::info!("Modbus Client ({}): Received QUIT command (no action needed).", socket_addr);
                                }
                            }
                        }
                        // Closure finished, but recv returned an error (channel closed/disconnected)
                        Ok(Err(e)) => {
                            log::warn!("Modbus Client ({}): Command channel (output_rx) closed or disconnected: {}. Exiting task.", socket_addr, e);
                            return Ok(()); // Graceful exit
                        }
                        // The blocking task itself failed (e.g., panicked)
                        Err(e) => {
                            log::error!("Modbus Client ({}): Blocking task for output_rx failed: {}. Breaking connection.", socket_addr, e);
                            break 'inner; // Treat as critical error, force reconnect attempt
                        }
                    }
                }

                 // Wrap the blocking crossbeam recv in spawn_blocking
                result = tokio::task::spawn_blocking(move || error_rx_c.recv()) => {
                     match result {
                        // Closure finished successfully, check its Result
                        Ok(Ok(())) => { // Received the error signal ()
                            log::warn!("Modbus Client ({}): Received error signal. Executing OFF sequence...", socket_addr);
                             match execute_inverter_off_sequence(&mut ctx, &socket_addr).await {
                                Ok(_) => { /* Success logged in helper */ }
                                Err(e) => {
                                    log::error!("Modbus Client ({}): OFF sequence failed after error signal: {}", socket_addr, e);
                                    break 'inner; // Reconnect on failure
                                }
                            }
                        }
                        // Closure finished, but recv returned an error (channel closed/disconnected)
                        Ok(Err(e)) => {
                             log::warn!("Modbus Client ({}): Error channel (error_rx) closed or disconnected: {}. Exiting task.", socket_addr, e);
                            return Ok(()); // Graceful exit
                        }
                        // The blocking task itself failed (e.g., panicked)
                        Err(e) => {
                             log::error!("Modbus Client ({}): Blocking task for error_rx failed: {}. Breaking connection.", socket_addr, e);
                            break 'inner; // Treat as critical error, force reconnect attempt
                        }
                    }
                }

                // Keep-alive check (unverändert)
                _ = sleep(Duration::from_secs(30)) => {
                     match ctx.read_holding_registers(40070, 1).await { // Example register
                        Ok(_) => { /* Connection seems okay */ }
                        Err(e) => {
                            log::error!("Modbus Client ({}): Keep-alive read failed: {}. Assuming disconnection.", socket_addr, e);
                            break 'inner; // Break inner loop to reconnect
                        }
                    }
                }
            } // end tokio::select!
        } // end inner loop (while connected)

        // Reconnect logic (unverändert)
        log::warn!(
            "Modbus Client ({}): Connection lost or error occurred. Reconnecting...",
            socket_addr
        );
    } // end outer loop (reconnection)
}