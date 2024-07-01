# Behringer FCB1010 programmer

This Rust application allows you to select a MIDI interface, send and receive SysEx messages, and edit the presets and global channels of the FCB1010 MIDI device. It includes a UI with a notebook view for easy editing of presets and a hexdump format.

## Features

- Select a MIDI interface from the available ports.
- Send and receive SysEx messages.
- Edit presets and global channels through an intuitive UI.
- View and edit the data in a hexdump format.
- Synchronize edits between the presets view and the hexdump view.
- Save and load SysEx data to/from a file.

## Installation

1. Ensure you have Rust and Cargo installed. If not, install them from [rust-lang.org](https://www.rust-lang.org/).
2. Clone the repository:
    ```sh
    git clone https://github.com/kurlytail/rust-fcb1010.git
    cd midi-interface-selector
    ```
3. Build the application:
    ```sh
    cargo build --release
    ```

## Usage

1. Run the application:
    ```sh
    cargo run --release
    ```
2. Select a MIDI interface from the dropdown menu.
3. Use the UI to send and receive SysEx messages, edit presets, and view/edit the hexdump.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please open an issue or submit a pull request on GitHub.

## Acknowledgments

This project uses the following crates:
- [egui](https://crates.io/crates/egui)
- [midir](https://crates.io/crates/midir)
- [serde](https://crates.io/crates/serde)
- [serde_json](https://crates.io/crates/serde_json)

## Author

- Your Name - [kurlytail](https://github.com/kurlytail)
