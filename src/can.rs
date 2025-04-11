// src/can.rs
use crate::{data::BmsData, error::AppError, SystemCommand};
use socketcan::{CanFilter, CanSocket, Frame, Socket, SocketOptions};
use std::{sync::{Arc, RwLock}, time::Duration};
use tokio::time::sleep; // Use tokio's sleep

// --- CAN Receiver Task ---
pub async fn rx_task(can_if: &str, bms_id: u8, bms_data: Arc<RwLock<Option<BmsData>>>, error_tx: crossbeam_channel::Sender<()>) -> Result<(), AppError> {
    log::info!("Starting CAN RX task for BMS ID {}", bms_id);

    // Open the CAN socket
    let socket = CanSocket::open(can_if)?;
    log::info!("Opened CAN socket on {} for BMS ID {}", can_if, bms_id);

    // Define CAN IDs to filter for based on bms_id
    let can_id1: u32 = if bms_id == 1 { 0xB101 } else { 0xB102 };
    let can_id2: u32 = if bms_id == 1 { 0xB201 } else { 0xB202 };

    // Set CAN filters
    // Standard Frame ID Mask (0x7FF for 11-bit IDs)
    // Use 0x1FFFFFFF for standard or extended frames if unsure
    let filters = [
        CanFilter::new(can_id1, 0x1FFFFFFF),
        CanFilter::new(can_id2, 0x1FFFFFFF),
    ];
    socket.set_filters(&filters)?;
    log::info!("Set CAN filters for IDs {:#X} and {:#X}", can_id1, can_id2);

    // Set non-blocking mode might be beneficial with async, but read_frame can block
    // socket.set_nonblocking(true)?;

    loop {
        match socket.read_frame() {
            Ok(frame) => {
                log::trace!("BMS {}: Received CAN frame: {:?}", bms_id, frame); // Use trace for verbose logging

                // Acquire write lock to update data
                match bms_data.write() {
                    Ok(mut data_guard) => {
                        // Get mutable reference, initializing if None
                        let data_ref = data_guard.get_or_insert_with(BmsData::default);
                        // Update data from the frame
                        if let Err(e) = data_ref.update_from_frame(&frame) {
                            log::error!("BMS {}: Failed to update data from CAN frame: {}", bms_id, e);
                        } else {
                             log::debug!("BMS {}: Successfully updated data for CAN ID {:#X}", bms_id, frame.raw_id());
                        }
                    }
                    Err(e) => {
                        log::error!("BMS {}: Failed to acquire write lock: {}", bms_id, e);
                        // Consider breaking or specific error handling for poisoned lock
                        return Err(AppError::LockPoisoned);
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No frame available right now (only relevant in non-blocking mode)
                // Yield control to the Tokio runtime
                tokio::task::yield_now().await;
                continue;
            }
            Err(e) => {
                // Handle other read errors (e.g., device unplugged)
                log::error!("BMS {}: Error reading from CAN socket: {}", bms_id, e);
                // Optional: add a delay before retrying or attempting to reopen
                sleep(Duration::from_secs(1)).await;
                // Potentially return the error to stop the task
                return Err(AppError::CanSocket(e));
            }
        }
        // Optional: yield to prevent tight loop if many frames arrive quickly
        // tokio::task::yield_now().await;
    }
    // Note: This loop currently runs forever. Add exit conditions if needed.
}


// --- CAN Transmitter Task (Placeholder) ---
pub async fn tx_task(can_if: &str, output_rx: crossbeam_channel::Receiver<SystemCommand>) -> Result<(), AppError> {
    log::info!("Starting CAN TX task (placeholder)");
    // In a real application, you would open a CANSocket here
    // let socket = CANSocket::open(can_if)?;
    loop {
        // Placeholder loop - implement sending logic if required
        // For example, read from a channel or queue and send frames:
        // let frame_to_send = ...;
        // socket.write_frame(&frame_to_send)?;
        log::debug!("CAN TX task running (no frames sent)");
        sleep(Duration::from_secs(10)).await; // Prevent busy-looping
    }
    // Ok(()) // Or return an error if the loop exits unexpectedly
}