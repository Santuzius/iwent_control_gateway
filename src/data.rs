// src/data.rs
use crate::error::AppError;
// Use the trait and the concrete type likely used by socketcan::CANSocket::read_frame
use socketcan::{CanFrame, Frame as CanFrameTrait, frame::AsPtr}; // Renamed Frame trait to avoid conflict
use std::convert::{TryFrom, TryInto};
use tokio_modbus::prelude::ExceptionCode; // For Modbus exceptions

// --- Constants for Modbus Register Mapping ---
// Copied from your provided snippet
pub const REG_MIN_CELL_VOLTAGE: u16 = 1;
pub const REG_MAX_CELL_VOLTAGE: u16 = 2;
pub const REG_MIN_TEMPERATURE: u16 = 3;
pub const REG_MAX_TEMPERATURE: u16 = 4;
pub const REG_BMS_INFO: u16 = 8;
pub const REG_SOC: u16 = 5;
// Message 2 (0xB20X) Data
pub const REG_CURRENT: u16 = 6;
pub const REG_TOTAL_VOLTAGE: u16 = 7;
pub const REG_WARNING_1: u16 = 9;
pub const REG_WARNING_2: u16 = 10;
pub const REG_ERROR_1: u16 = 11;
pub const REG_ERROR_2: u16 = 12;
// Writeable registers
pub const REG_ON: u16 = 21;
pub const REG_QUIT: u16 = 22;

// --- BmsData Struct ---
#[derive(Debug, Clone, Default)]
pub struct BmsData {
    // Raw integer values directly from CAN or scaled for Modbus
    pub min_cell_voltage: Option<u16>,
    pub max_cell_voltage: Option<u16>,
    pub min_temperature: Option<u8>,
    pub max_temperature: Option<u8>,
    pub info: Option<u8>,
    pub soc: Option<u8>,
    pub current: Option<u16>, // Kept as u16 as per user code. Note: May not represent negative current easily.
    pub total_voltage: Option<u16>,
    pub warning1: Option<u8>,
    pub warning2: Option<u8>,
    pub error1: Option<u8>,
    pub error2: Option<u8>,
    // Fields to store values written via Modbus
    pub on: Option<u8>,
    pub quit: Option<u8>,
}

impl BmsData {
    // Function to update data from a CAN frame
    // Changed signature back to CANFrame for consistency with previous example and socketcan="2.0" CANSocket
    pub fn update_from_frame(&mut self, frame: &CanFrame) -> Result<(), AppError> {
        let can_id = frame.raw_id(); // Use id() method
        let data = frame.as_bytes(); // Use data() method

        match can_id {
            0xB101 | 0xB102 => {
                // Message 1 processing
                if data.len() != 8 {
                    return Err(AppError::InvalidCanDataLength {
                        can_id,
                        expected: 8,
                        actual: data.len(),
                    });
                }
                // Min. cell voltage (data0, data1) - Little Endian
                self.min_cell_voltage = Some(u16::from_le_bytes(data[0..2].try_into().unwrap()));
                // Max. cell voltage (data2, data3) - Little Endian
                self.max_cell_voltage = Some(u16::from_le_bytes(data[2..4].try_into().unwrap()));
                // Min. temperature (data4)
                self.min_temperature = Some(data[4]);
                // Max. temperature (data5)
                self.max_temperature = Some(data[5]);
                // Info (data6)
                self.info = Some(data[6]);
                // SOC (data7)
                self.soc = Some(data[7]);
                log::debug!("Processed CAN ID {:#X} (Type 1)", can_id);
            }
            0xB201 | 0xB202 => {
                // Message 2 processing
                if data.len() != 8 {
                    return Err(AppError::InvalidCanDataLength {
                        can_id,
                        expected: 8,
                        actual: data.len(),
                    });
                }
                // Current (data0, data1) - Little Endian
                self.current = Some(u16::from_le_bytes(data[0..2].try_into().unwrap()));
                // Total voltage (data2, data3) - Little Endian
                self.total_voltage = Some(u16::from_le_bytes(data[2..4].try_into().unwrap()));
                // Warning 1 (data4)
                self.warning1 = Some(data[4]);
                // Warning 2 (data5)
                self.warning2 = Some(data[5]);
                // Error 1 (data6)
                self.error1 = Some(data[6]);
                // Error 2 (data7)
                self.error2 = Some(data[7]);
                log::debug!("Processed CAN ID {:#X} (Type 2)", can_id);
            }
            _ => {
                // This shouldn't happen if filters are set correctly, but good practice
                return Err(AppError::UnsupportedCanId(can_id));
            }
        }
        Ok(())
    }

    // Function to get data for a specific Modbus register (READ)
    pub fn get_register(&self, address: u16) -> Option<u16> {
        match address {
            REG_MIN_CELL_VOLTAGE => self.min_cell_voltage,
            REG_MAX_CELL_VOLTAGE => self.max_cell_voltage,
            REG_MIN_TEMPERATURE => self.min_temperature.map(u16::from),
            REG_MAX_TEMPERATURE => self.max_temperature.map(u16::from),
            REG_BMS_INFO => Some(self.info.map(u16::from).unwrap_or(0xFF)),
            REG_SOC => self.soc.map(u16::from),
            REG_CURRENT => self.current,
            REG_TOTAL_VOLTAGE => self.total_voltage,
            REG_WARNING_1 => self.warning1.map(u16::from),
            REG_WARNING_2 => self.warning2.map(u16::from),
            REG_ERROR_1 => Some(self.info.map(u16::from).unwrap_or(0xFF)),
            REG_ERROR_2 => Some(self.info.map(u16::from).unwrap_or(0xFF)),
            // Read back the values written via Modbus
            REG_ON => self.on.map(u16::from),
            REG_QUIT => self.quit.map(u16::from),
            _ => None, // Address out of defined range or not readable
        }
    }

    // Function to set data for a specific Modbus register (WRITE)
    pub fn set_register(&mut self, address: u16, value: u16) -> Result<(), ExceptionCode> {
        match address {
            REG_ON => {
                // Validate if the u16 value fits into u8
                match u8::try_from(value) {
                    Ok(val_u8) => {
                        self.on = Some(val_u8);
                        log::info!("Set REG_ON (addr {}) to {}", address, val_u8);
                        Ok(())
                    }
                    Err(_) => {
                        log::warn!(
                            "Modbus write to REG_ON (addr {}): Value {} out of range for u8.",
                            address,
                            value
                        );
                        Err(ExceptionCode::IllegalDataValue)
                    }
                }
            }
            REG_QUIT => {
                // Validate if the u16 value fits into u8
                match u8::try_from(value) {
                    Ok(val_u8) => {
                        self.quit = Some(val_u8);
                        log::info!("Set REG_QUIT (addr {}) to {}", address, val_u8);
                        Ok(())
                    }
                    Err(_) => {
                        log::warn!(
                            "Modbus write to REG_QUIT (addr {}): Value {} out of range for u8.",
                            address,
                            value
                        );
                        Err(ExceptionCode::IllegalDataValue)
                    }
                }
            }
            // Add other writable registers here if needed in the future

            // If the address is known but not writable
            REG_MIN_CELL_VOLTAGE | REG_MAX_CELL_VOLTAGE | REG_MIN_TEMPERATURE
            | REG_MAX_TEMPERATURE | REG_BMS_INFO | REG_SOC | REG_CURRENT | REG_TOTAL_VOLTAGE
            | REG_WARNING_1 | REG_WARNING_2 | REG_ERROR_1 | REG_ERROR_2 => {
                log::warn!("Attempted write to read-only register address {}", address);
                Err(ExceptionCode::IllegalFunction) // Or IllegalDataAddress
            }

            // If the address is completely unknown/invalid
            _ => {
                log::warn!("Attempted write to unknown register address {}", address);
                Err(ExceptionCode::IllegalDataAddress)
            }
        }
    }
}
