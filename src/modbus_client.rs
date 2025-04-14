// src/modbus_client.rs
use crate::error::AppError;
use crate::SystemCommand;
use std::{net::SocketAddr, time::Duration};
use tokio::net::TcpStream;
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
        let _ = ctx.write_single_register(*reg, *val).await?;
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
    error_rx: crossbeam_channel::Receiver<()>,
    output_rx: crossbeam_channel::Receiver<SystemCommand>,
) -> Result<(), AppError> {
    let socket_addr: SocketAddr = addr_str.parse().map_err(|e| {
        log::error!("Invalid socket address format '{}': {}", addr_str, e);
    }).unwrap();

    log::info!("Starting Modbus TCP client task for {}", socket_addr);

    // Flag, um zu verfolgen, ob der error_rx-Kanal geschlossen ist
    let mut error_rx_closed = false;

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
            tokio::select! {
                biased; // Prioritize receiving commands/errors over keep-alive

                // --- output_rx branch ---
                // Klonen und in spawn_blocking verschieben
                result = { let rx = output_rx.clone(); tokio::task::spawn_blocking(move || rx.recv()) } => {
                    match result {
                        Ok(Ok(command)) => {
                            log::debug!("Modbus Client ({}): Received command: {:?}", socket_addr, command);
                            match command {
                                SystemCommand::Off => {
                                    match execute_inverter_off_sequence(&mut ctx, &socket_addr).await {
                                        Ok(_) => { /* Success logged */ }
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
                        Ok(Err(e)) => {
                            // Wenn der *Befehlskanal* schließt, wollen wir wahrscheinlich beenden.
                            log::warn!("Modbus Client ({}): Command channel (output_rx) closed or disconnected: {}. Exiting task.", socket_addr, e);
                            return Ok(()); // Task beenden, da keine Befehle mehr kommen können
                        }
                        Err(e) => {
                            log::error!("Modbus Client ({}): Blocking task for output_rx failed: {}. Breaking connection.", socket_addr, e);
                            break 'inner; // Kritischer Fehler -> Reconnect
                        }
                    }
                }

                // --- error_rx branch (nur pollen, wenn nicht geschlossen) ---
                // Syntax: future = ..., if condition
                result = { let rx = error_rx.clone(); tokio::task::spawn_blocking(move || rx.recv()) }, if !error_rx_closed => {
                     match result {
                        Ok(Ok(())) => { // Signal empfangen
                            log::warn!("Modbus Client ({}): Received error signal. Executing OFF sequence...", socket_addr);
                             match execute_inverter_off_sequence(&mut ctx, &socket_addr).await {
                                Ok(_) => { /* Success logged */ }
                                Err(e) => {
                                    log::error!("Modbus Client ({}): OFF sequence failed after error signal: {}", socket_addr, e);
                                    // Brechen Sie die Verbindung ab, wenn die *Ausführung* der OFF-Sequenz fehlschlägt
                                    break 'inner;
                                }
                            }
                        }
                        Ok(Err(e)) => { // Kanal wurde geschlossen (recv-Fehler)
                             log::warn!("Modbus Client ({}): Error channel (error_rx) closed or disconnected: {}. Will stop listening on this channel.", socket_addr, e);
                             // Setze Flag, damit dieser Zweig nicht mehr abgefragt wird
                             error_rx_closed = true;
                             // NICHT `return` oder `break`! Einfach weiterlaufen lassen.
                        }
                        Err(e) => { // spawn_blocking-Task selbst fehlgeschlagen
                             log::error!("Modbus Client ({}): Blocking task for error_rx failed: {}. Assuming channel unusable.", socket_addr, e);
                             // Setze Flag, damit dieser Zweig nicht mehr abgefragt wird
                             error_rx_closed = true;
                             // NICHT `return` oder `break`! Einfach weiterlaufen lassen.
                        }
                    }
                }

                // --- Keep-alive branch (unverändert) ---
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
        // Ggf. kurze Pause vor dem Neuverbindungsversuch einfügen
        // sleep(Duration::from_secs(1)).await;
    } // end outer loop (reconnection)
}