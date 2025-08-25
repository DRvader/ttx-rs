use ttx_rs::{
    chip::noc::{NocInterface, Tile},
    kernel::KernelData,
};

fn forward_to_logger(frame: &defmt_decoder::Frame, location_info: LocationInfo) {
    let (file, line, mod_path) = location_info;
    defmt_decoder::log::log_defmt(frame, file.as_deref(), line, mod_path.as_deref());
}

type LocationInfo = (Option<String>, Option<u32>, Option<String>);

fn location_info(
    locs: &Option<defmt_decoder::Locations>,
    frame: &defmt_decoder::Frame,
    current_dir: &std::path::Path,
) -> LocationInfo {
    let (mut file, mut line, mut mod_path) = (None, None, None);

    let loc = locs.as_ref().map(|locs| locs.get(&frame.index()));

    if let Some(Some(loc)) = loc {
        // try to get the relative path, else the full one
        let path = loc.file.strip_prefix(current_dir).unwrap_or(&loc.file);

        file = Some(path.display().to_string());
        line = Some(loc.line as u32);
        mod_path = Some(loc.module.clone());
    }

    (file, line, mod_path)
}

pub fn run_defmt(mut log_chip: ttx_rs::Chip, tile: Tile, elf: &[u8], data: &KernelData) {
    let verbose = false;

    let table = defmt_decoder::Table::parse(elf).unwrap();
    let table = match table {
        Some(table) => table,
        None => {
            tracing::warn!("Did not fine .defmt section in elf bytes");
            return;
        }
    };

    let locs = table.get_locations(elf).unwrap();
    let locs = if table.indices().all(|idx| locs.contains_key(&(idx as u64))) {
        Some(locs)
    } else {
        tracing::warn!("(BUG) location info is incomplete; it will be omitted from the output");
        return;
    };

    let mut formatter_config = defmt_decoder::log::format::FormatterConfig::default();
    formatter_config.is_timestamp_available = table.has_timestamp();

    let host_formatter_config = defmt_decoder::log::format::FormatterConfig::default();

    let formatter = defmt_decoder::log::format::Formatter::new(formatter_config);
    let host_formatter = defmt_decoder::log::format::HostFormatter::new(host_formatter_config);

    let current_dir = std::env::current_dir().unwrap();

    let mut stream_decoder = table.new_stream_decoder();
    loop {
        let read = log_chip.noc_read32(ttx_rs::chip::noc::NocId::Noc1, tile, data["LOG_READ"]);
        let write = log_chip.noc_read32(ttx_rs::chip::noc::NocId::Noc1, tile, data["LOG_WRITE"]);

        if read == write {
            continue;
        }

        let mut value = [0u8];
        log_chip.noc_read(
            ttx_rs::chip::noc::NocId::Noc1,
            tile,
            data["LOG_BUFFER"] + read as u64,
            &mut value,
        );

        log_chip.noc_write32(
            ttx_rs::chip::noc::NocId::Noc1,
            tile,
            data["LOG_READ"],
            (read + 1) % 1024,
        );

        stream_decoder.received(&value);

        loop {
            match stream_decoder.decode() {
                Ok(frame) => forward_to_logger(&frame, location_info(&locs, &frame, &current_dir)),
                Err(defmt_decoder::DecodeError::UnexpectedEof) => break,
                Err(defmt_decoder::DecodeError::Malformed) => break,
            }
        }
    }
}
