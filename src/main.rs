use eframe::egui;
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

const CONFIG_FILE: &str = "config.json";
const SYSEX_FILE: &str = "preset_data.syx";

#[derive(Serialize, Deserialize, Default)]
struct AppConfig {
    selected_port: Option<usize>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
pub struct Preset {
    program_changes: [u8; 5],
    control_changes: [(u8, u8); 2],
    expression_pedal_a: (u8, u8, u8),
    expression_pedal_b: (u8, u8, u8),
    note: u8,
}

impl Preset {
    pub fn new() -> Self {
        Self {
            program_changes: [0; 5],
            control_changes: [(0, 0); 2],
            expression_pedal_a: (0, 0, 0),
            expression_pedal_b: (0, 0, 0),
            note: 0,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            program_changes: [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]],
            control_changes: [(bytes[5], bytes[6]), (bytes[7], bytes[8])],
            expression_pedal_a: (bytes[9], bytes[10], bytes[11]),
            expression_pedal_b: (bytes[12], bytes[13], bytes[14]),
            note: bytes[15],
        }
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        [
            self.program_changes[0],
            self.program_changes[1],
            self.program_changes[2],
            self.program_changes[3],
            self.program_changes[4],
            self.control_changes[0].0,
            self.control_changes[0].1,
            self.control_changes[1].0,
            self.control_changes[1].1,
            self.expression_pedal_a.0,
            self.expression_pedal_a.1,
            self.expression_pedal_a.2,
            self.expression_pedal_b.0,
            self.expression_pedal_b.1,
            self.expression_pedal_b.2,
            self.note,
        ]
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct SysExMessage {
    start_byte: u8,
    manufacturer_id: [u8; 3],
    global_channel: u8,
    device_id: u8,
    #[serde(with = "serde_arrays")]
    presets: [Preset; 100],
    #[serde(with = "serde_arrays")]
    global_channels: [u8; 10],
    end_byte: u8,
    original_data: Option<Vec<u8>>,
}

impl Default for SysExMessage {
    fn default() -> Self {
        Self {
            start_byte: 0xf0,
            manufacturer_id: [0x00, 0x20, 0x32],
            global_channel: 0x00,
            device_id: 0x0c,
            presets: [Preset::new(); 100],
            global_channels: [0; 10],
            end_byte: 0xf7,
            original_data: None,
        }
    }
}

impl SysExMessage {
    pub fn encode(&self) -> Vec<u8> {
        let mut encoded: Vec<u8> = Vec::new();
        encoded.push(self.start_byte);
        encoded.extend_from_slice(&self.manufacturer_id);
        encoded.push(self.global_channel);
        encoded.push(self.device_id);
        encoded.push(0x0f); // Hacked patch

        let mut patched_data: Vec<u8> = if let Some(ref data) = self.original_data {
            data[7..data.len() - 1].to_vec()
        } else {
            vec![0u8; 0x7ea] // Size to cover the entire data area including global channels
        };

        // Encode the presets and global channels into the patched data
        for (i, preset) in self.presets.iter().enumerate() {
            let bytes = preset.to_bytes();
            for (j, &byte) in bytes.iter().enumerate() {
                patched_data[i * 16 + j] = byte;
            }
        }

        for (i, &channel) in self.global_channels.iter().enumerate() {
            patched_data[0x7e0 + i] = channel;
        }

        // Perform 8-bit to 7-bit encoding
        let mut index = 0;
        while index < patched_data.len() {
            let chunk = &patched_data[index..index + 7.min(patched_data.len() - index)];
            let mut data: [u8; 8] = [0; 8];
            let mut msb_byte = 0u8;
            for (i, &byte) in chunk.iter().enumerate() {
                msb_byte |= (byte >> 7) << i;
                data[i] = byte & 0x7F;
            }
            data[7] = msb_byte;
            encoded.extend_from_slice(&data);
            index += 7;
        }

        encoded.push(self.end_byte);
        encoded
    }

    pub fn decode(data: &[u8]) -> Result<Self, MidiError> {
        if data.len() < 6 {
            return Err(MidiError::InvalidDataLength);
        }

        if data[0] != 0xf0 {
            return Err(MidiError::InvalidSysExStart);
        }

        if data[data.len() - 1] != 0xf7 {
            return Err(MidiError::InvalidSysExEnd);
        }

        let manufacturer_id = [data[1], data[2], data[3]];
        let global_channel = data[4];
        let device_id = data[5];

        let mut fixed_data: Vec<u8> = Vec::new();
        let mut index = 7;

        while index + 8 <= data.len() - 1 {
            let chunk = &data[index..index + 8];
            let msb_byte = chunk[7];
            for i in 0..7 {
                let byte = chunk[i] | ((msb_byte >> i) & 0x01) << 7;
                fixed_data.push(byte);
            }
            index += 8;
        }

        // Let's hexdump the fixed_data for debugging
        eprintln!("{}", hexdump(&fixed_data));

        let mut presets: [Preset; 100] = unsafe { std::mem::zeroed() };
        let mut preset_bytes: Vec<u8> = Vec::new();
        let mut preset_index = 0;

        for byte in &fixed_data[0..0x640] {
            preset_bytes.push(*byte);
            if preset_bytes.len() == 16 {
                presets[preset_index] = Preset::from_bytes(&preset_bytes);
                preset_index += 1;
                preset_bytes.clear();
            }
        }

        // Assuming the global MIDI channel data starts at address 0x7e0
        let mut global_channels: [u8; 10] = [0; 10];
        global_channels.copy_from_slice(&fixed_data[0x7e0..0x7ea]);

        Ok(Self {
            start_byte: 0xf0,
            manufacturer_id,
            global_channel,
            device_id,
            presets,
            global_channels,
            original_data: Some(data.to_vec()), // Save the original data
            end_byte: 0xf7,
        })
    }
}

#[derive(Debug)]
pub enum MidiError {
    InvalidSysExStart,
    InvalidSysExEnd,
    InvalidDataLength,
}

struct MidiApp {
    available_ports: Vec<String>,
    selected_port: Option<usize>,
    midi_out_connection: Option<MidiOutputConnection>,
    midi_in_connection: Option<MidiInputConnection<()>>,
    config: AppConfig,
    sysex_message: Arc<Mutex<SysExMessage>>,
    receiving_sysex: Arc<Mutex<bool>>,
}

impl Default for MidiApp {
    fn default() -> Self {
        let midi_in = MidiInput::new("MIDI Input").unwrap();
        let available_ports = midi_in
            .ports()
            .iter()
            .map(|p| midi_in.port_name(p).unwrap())
            .collect();

        let config: AppConfig = if let Ok(config_str) = fs::read_to_string(CONFIG_FILE) {
            serde_json::from_str(&config_str).unwrap_or_default()
        } else {
            AppConfig::default()
        };

        let selected_port = config.selected_port;
        let midi_out_connection = if let Some(port_index) = selected_port {
            let midi_out = MidiOutput::new("MIDI Output").unwrap();
            let port = midi_out.ports().get(port_index).cloned();
            port.and_then(|p| midi_out.connect(&p, "midir-test").ok())
        } else {
            None
        };

        let sysex_message: SysExMessage = if let Ok(sysex_str) = fs::read_to_string(SYSEX_FILE) {
            serde_json::from_str(&sysex_str).unwrap_or_default()
        } else {
            SysExMessage::default()
        };

        Self {
            available_ports,
            selected_port,
            midi_out_connection,
            midi_in_connection: None,
            config,
            sysex_message: Arc::new(Mutex::new(sysex_message)),
            receiving_sysex: Arc::new(Mutex::new(false)),
        }
    }
}

impl eframe::App for MidiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("MIDI Interface Selector");

            egui::ComboBox::from_label("Select MIDI Interface")
                .selected_text(self.selected_port.map_or("None".to_string(), |index| {
                    self.available_ports[index].clone()
                }))
                .show_ui(ui, |ui| {
                    for (index, port) in self.available_ports.iter().enumerate() {
                        if ui
                            .selectable_value(&mut self.selected_port, Some(index), port)
                            .clicked()
                        {
                            if let Some(port_index) = self.selected_port {
                                let midi_out = MidiOutput::new("MIDI Output").unwrap();
                                let port = midi_out.ports().get(port_index).cloned();
                                self.midi_out_connection =
                                    port.and_then(|p| midi_out.connect(&p, "midir-test").ok());

                                self.config.selected_port = Some(port_index);
                                if let Ok(config_str) = serde_json::to_string(&self.config) {
                                    fs::write(CONFIG_FILE, config_str).ok();
                                }
                            }
                        }
                    }
                });

            if let Some(selected_index) = self.selected_port {
                ui.label(format!(
                    "Selected: {}",
                    self.available_ports[selected_index]
                ));
            } else {
                ui.label("No MIDI interface selected");
            }

            ui.separator();

            if ui.button("Save to SysEx").clicked() {
                if let Ok(sysex_str) = serde_json::to_string(&*self.sysex_message.lock().unwrap()) {
                    fs::write(SYSEX_FILE, sysex_str).ok();
                    ui.label("SysEx data saved");
                } else {
                    ui.label("Failed to save SysEx data");
                }
            }

            if ui.button("Load from SysEx").clicked() {
                if let Ok(sysex_str) = fs::read_to_string(SYSEX_FILE) {
                    *self.sysex_message.lock().unwrap() =
                        serde_json::from_str(&sysex_str).unwrap_or_default();
                    ui.label("SysEx data loaded");
                } else {
                    ui.label("Failed to load SysEx data");
                }
            }

            if ui.button("Send SysEx Message").clicked() {
                if let Some(connection) = &mut self.midi_out_connection {
                    let message = self.sysex_message.lock().unwrap().encode();
                    connection.send(&message).unwrap();
                    ui.label("SysEx message sent");
                } else {
                    ui.label("No MIDI connection available");
                }
            }

            if ui.button("Receive SysEx Message").clicked() {
                if let Some(port_index) = self.selected_port {
                    let midi_in = MidiInput::new("MIDI Input").unwrap();
                    let port = midi_in.ports().get(port_index).cloned();
                    if let Some(port) = port {
                        let (sender, receiver) = channel();
                        let connection = midi_in
                            .connect(
                                &port,
                                "midir-read-input",
                                move |_, message, _| {
                                    eprintln!("Received:\n{}", hexdump(message));
                                    if message[0] == 0xF0 && message[message.len() - 1] == 0xF7 {
                                        sender.send(message.to_vec()).unwrap();
                                    }
                                },
                                (),
                            )
                            .unwrap();

                        self.midi_in_connection = Some(connection);
                        *self.receiving_sysex.lock().unwrap() = true;

                        let ctx_clone = ctx.clone();
                        let sysex_message_clone = Arc::clone(&self.sysex_message);
                        let receiving_sysex_clone = Arc::clone(&self.receiving_sysex);

                        std::thread::spawn(move || {
                            if let Ok(message) = receiver.recv() {
                                if let Ok(sysex_message) = SysExMessage::decode(&message) {
                                    *sysex_message_clone.lock().unwrap() = sysex_message;
                                    *receiving_sysex_clone.lock().unwrap() = false;
                                    ctx_clone.request_repaint();
                                }
                            }
                        });
                    }
                }
            }

            ui.separator();
            ui.heading("Presets");

            let columns = 5; // Number of presets per row

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("preset_grid").show(ui, |ui| {
                    for (i, preset) in self
                        .sysex_message
                        .lock()
                        .unwrap()
                        .presets
                        .iter_mut()
                        .enumerate()
                    {
                        if i % columns == 0 && i != 0 {
                            ui.end_row();
                        }

                        ui.group(|ui| {
                            ui.label(format!("Preset {}", i + 1));

                            for (j, program_change) in preset.program_changes.iter_mut().enumerate()
                            {
                                ui.horizontal(|ui| {
                                    ui.label(format!("PC {}:", j + 1));
                                    ui.add(
                                        egui::DragValue::new(program_change)
                                            .speed(0.1)
                                            .clamp_range(0..=127),
                                    );
                                });
                            }

                            for (j, (control_change, value)) in
                                preset.control_changes.iter_mut().enumerate()
                            {
                                ui.horizontal(|ui| {
                                    ui.label(format!("CC {}:", j + 1));
                                    ui.add(
                                        egui::DragValue::new(control_change)
                                            .speed(0.1)
                                            .clamp_range(0..=127),
                                    );
                                    ui.label("Value:");
                                    ui.add(
                                        egui::DragValue::new(value).speed(0.1).clamp_range(0..=127),
                                    );
                                });
                            }

                            ui.horizontal(|ui| {
                                ui.label("EP A:");
                                ui.add(
                                    egui::DragValue::new(&mut preset.expression_pedal_a.0)
                                        .speed(0.1)
                                        .clamp_range(0..=127),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut preset.expression_pedal_a.1)
                                        .speed(0.1)
                                        .clamp_range(0..=127),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut preset.expression_pedal_a.2)
                                        .speed(0.1)
                                        .clamp_range(0..=127),
                                );
                            });

                            ui.horizontal(|ui| {
                                ui.label("EP B:");
                                ui.add(
                                    egui::DragValue::new(&mut preset.expression_pedal_b.0)
                                        .speed(0.1)
                                        .clamp_range(0..=127),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut preset.expression_pedal_b.1)
                                        .speed(0.1)
                                        .clamp_range(0..=127),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut preset.expression_pedal_b.2)
                                        .speed(0.1)
                                        .clamp_range(0..=127),
                                );
                            });

                            ui.horizontal(|ui| {
                                ui.label("Note:");
                                ui.add(
                                    egui::DragValue::new(&mut preset.note)
                                        .speed(0.1)
                                        .clamp_range(0..=127),
                                );
                            });
                        });
                    }
                });
            });

            if *self.receiving_sysex.lock().unwrap() {
                egui::Window::new("Receiving SysEx")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Receiving SysEx message...");
                        if ui.button("Cancel").clicked() {
                            *self.receiving_sysex.lock().unwrap() = false;
                            self.midi_in_connection = None;
                        }
                    });
            }
        });
    }
}

fn hexdump(data: &[u8]) -> String {
    let mut result = String::new();
    for (i, chunk) in data.chunks(16).enumerate() {
        result.push_str(&format!("{:08x}: ", i * 16));
        for byte in chunk {
            result.push_str(&format!("{:02x} ", byte));
        }
        for _ in 0..(16 - chunk.len()) {
            result.push_str("   ");
        }
        result.push_str("  ");
        for byte in chunk {
            let ch = if byte.is_ascii_graphic() {
                *byte as char
            } else {
                '.'
            };
            result.push(ch);
        }
        result.push('\n');
    }
    result
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "MIDI Interface Selector",
        options,
        Box::new(|_cc| Box::<MidiApp>::default()),
    )
}
