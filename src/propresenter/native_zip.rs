//! Stored-only ZIP64 writer for native `ProPresenter` playlist packages.

use std::fs::File;
use std::io::{self, Seek, Write};

const LOCAL_FILE_HEADER: u32 = 0x0403_4b50;
const CENTRAL_DIRECTORY_HEADER: u32 = 0x0201_4b50;
const ZIP64_END_OF_CENTRAL_DIRECTORY: u32 = 0x0606_4b50;
const ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR: u32 = 0x0706_4b50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
const ZIP64_VERSION: u16 = 45;
const UNIX_VERSION_3_0: u16 = 0x031e;
const ZIP64_EXTRA_FIELD_LENGTH: u16 = 28;
const DOS_1980_01_01: u16 = 0x0021;
const NATIVE_END_RECORDS_SIZE: u64 = 98;

/// One member of a native playlist archive.
pub(super) struct Entry<'a> {
    name: String,
    data: &'a [u8],
}

impl<'a> Entry<'a> {
    /// Create an entry borrowing bytes owned by the playlist build.
    pub(super) const fn borrowed(name: String, data: &'a [u8]) -> Self {
        Self { name, data }
    }
}

struct IndexEntry {
    name: String,
    size: u64,
    crc32: u32,
    local_header_offset: u64,
}

/// Write the ZIP64 shape observed across native `ProPresenter` exports.
///
/// Every member is stored, every local and central record carries forced
/// ZIP64 metadata, and the archive always ends with ZIP64 end records plus a
/// legacy end record. Physical member order is lexicographic across the whole
/// archive, including the literal `data` entry.
pub(super) fn write(mut file: File, mut entries: Vec<Entry<'_>>) -> io::Result<File> {
    entries.sort_by(|left, right| left.name.cmp(&right.name));

    let mut index = Vec::with_capacity(entries.len());
    for entry in entries {
        let name = entry.name.as_bytes();
        let name_length = u16::try_from(name.len())
            .map_err(|_| invalid_input(format!("archive path is too long: {:?}", entry.name)))?;
        let size = u64::try_from(entry.data.len())
            .map_err(|_| invalid_input("archive member is too large"))?;
        let local_header_offset = file.stream_position()?;
        let crc32 = crc32fast::hash(entry.data);
        write_u32(&mut file, LOCAL_FILE_HEADER)?;
        write_u16(&mut file, ZIP64_VERSION)?;
        write_u16(&mut file, 0)?; // native exports leave the UTF-8 flag unset
        write_u16(&mut file, 0)?; // stored
        write_u16(&mut file, 0)?; // volatile timestamp intentionally omitted
        write_u16(&mut file, DOS_1980_01_01)?;
        write_u32(&mut file, crc32)?;
        write_u32(&mut file, u32::MAX)?;
        write_u32(&mut file, u32::MAX)?;
        write_u16(&mut file, name_length)?;
        write_u16(&mut file, ZIP64_EXTRA_FIELD_LENGTH)?;
        file.write_all(name)?;
        write_zip64_extra(&mut file, size, local_header_offset)?;
        file.write_all(entry.data)?;

        index.push(IndexEntry {
            name: entry.name,
            size,
            crc32,
            local_header_offset,
        });
    }

    let central_directory_offset = file.stream_position()?;
    for entry in &index {
        let name = entry.name.as_bytes();
        let name_length = u16::try_from(name.len())
            .map_err(|_| invalid_input(format!("archive path is too long: {:?}", entry.name)))?;

        write_u32(&mut file, CENTRAL_DIRECTORY_HEADER)?;
        write_u16(&mut file, UNIX_VERSION_3_0)?;
        write_u16(&mut file, ZIP64_VERSION)?;
        write_u16(&mut file, 0)?; // native exports leave the UTF-8 flag unset
        write_u16(&mut file, 0)?; // stored
        write_u16(&mut file, 0)?;
        write_u16(&mut file, DOS_1980_01_01)?;
        write_u32(&mut file, entry.crc32)?;
        write_u32(&mut file, u32::MAX)?;
        write_u32(&mut file, u32::MAX)?;
        write_u16(&mut file, name_length)?;
        write_u16(&mut file, ZIP64_EXTRA_FIELD_LENGTH)?;
        write_u16(&mut file, 0)?; // comment length
        write_u16(&mut file, 0)?; // disk number
        write_u16(&mut file, 0)?; // internal attributes
        write_u32(&mut file, 0)?; // external attributes
        write_u32(
            &mut file,
            u32::try_from(entry.local_header_offset).unwrap_or(u32::MAX),
        )?;
        file.write_all(name)?;
        write_zip64_extra(&mut file, entry.size, entry.local_header_offset)?;
    }

    let central_directory_end = file.stream_position()?;
    let central_directory_size = central_directory_end - central_directory_offset;
    // ProPresenter includes its 56-byte ZIP64 end record, 20-byte locator,
    // and 22-byte legacy end record in both central-directory size fields.
    // This differs from the ZIP specification but is uniform in native exports.
    let native_central_directory_size = central_directory_size
        .checked_add(NATIVE_END_RECORDS_SIZE)
        .ok_or_else(|| invalid_input("central directory is too large"))?;
    let entry_count =
        u64::try_from(index.len()).map_err(|_| invalid_input("too many archive members"))?;

    write_u32(&mut file, ZIP64_END_OF_CENTRAL_DIRECTORY)?;
    write_u64(&mut file, 44)?;
    write_u16(&mut file, UNIX_VERSION_3_0)?;
    write_u16(&mut file, ZIP64_VERSION)?;
    write_u32(&mut file, 0)?;
    write_u32(&mut file, 0)?;
    write_u64(&mut file, entry_count)?;
    write_u64(&mut file, entry_count)?;
    write_u64(&mut file, native_central_directory_size)?;
    write_u64(&mut file, central_directory_offset)?;

    write_u32(&mut file, ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR)?;
    write_u32(&mut file, 0)?;
    write_u64(&mut file, central_directory_end)?;
    write_u32(&mut file, 1)?;

    write_u32(&mut file, END_OF_CENTRAL_DIRECTORY)?;
    write_u16(&mut file, 0)?;
    write_u16(&mut file, 0)?;
    write_u16(&mut file, u16::try_from(entry_count).unwrap_or(u16::MAX))?;
    write_u16(&mut file, u16::try_from(entry_count).unwrap_or(u16::MAX))?;
    write_u32(
        &mut file,
        u32::try_from(native_central_directory_size).unwrap_or(u32::MAX),
    )?;
    write_u32(
        &mut file,
        u32::try_from(central_directory_offset).unwrap_or(u32::MAX),
    )?;
    write_u16(&mut file, 0)?;

    Ok(file)
}

fn write_zip64_extra(
    writer: &mut impl Write,
    size: u64,
    local_header_offset: u64,
) -> io::Result<()> {
    write_u16(writer, 0x0001)?;
    write_u16(writer, 24)?;
    write_u64(writer, size)?;
    write_u64(writer, size)?;
    write_u64(writer, local_header_offset)
}

fn write_u16(writer: &mut impl Write, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32(writer: &mut impl Write, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u64(writer: &mut impl Write, value: u64) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn writes_native_forced_zip64_records_and_global_member_order() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("native.proplaylist");
        let file = File::create(&path).expect("create archive");
        let file = write(
            file,
            vec![
                Entry::borrowed("{Zulu}.pro".to_string(), b"z"),
                Entry::borrowed("data".to_string(), b"d"),
                Entry::borrowed("Alpha.pro".to_string(), b"a"),
            ],
        )
        .expect("write archive");
        drop(file);

        let bytes = std::fs::read(&path).expect("read archive");
        assert_eq!(read_u32(&bytes, 0), LOCAL_FILE_HEADER);
        assert_eq!(read_u16(&bytes, 4), ZIP64_VERSION);
        assert_eq!(read_u32(&bytes, 18), u32::MAX);
        assert_eq!(read_u32(&bytes, 22), u32::MAX);
        let local_name_length = usize::from(read_u16(&bytes, 26));
        assert_eq!(read_u16(&bytes, 28), 28);
        assert_eq!(&bytes[30..30 + local_name_length], b"Alpha.pro");
        let local_extra = 30 + local_name_length;
        assert_eq!(read_u16(&bytes, local_extra), 0x0001);
        assert_eq!(read_u16(&bytes, local_extra + 2), 24);
        assert_eq!(read_u64(&bytes, local_extra + 4), 1);
        assert_eq!(read_u64(&bytes, local_extra + 12), 1);
        assert_eq!(read_u64(&bytes, local_extra + 20), 0);

        let central = bytes
            .windows(4)
            .position(|window| window == CENTRAL_DIRECTORY_HEADER.to_le_bytes())
            .expect("central directory");
        assert_eq!(read_u16(&bytes, central + 4), UNIX_VERSION_3_0);
        assert_eq!(read_u16(&bytes, central + 6), ZIP64_VERSION);
        assert_eq!(read_u32(&bytes, central + 20), u32::MAX);
        assert_eq!(read_u32(&bytes, central + 24), u32::MAX);
        assert_eq!(read_u16(&bytes, central + 30), 28);
        assert_eq!(read_u32(&bytes, central + 38), 0);
        let central_name_length = usize::from(read_u16(&bytes, central + 28));
        let central_extra = central + 46 + central_name_length;
        assert_eq!(read_u16(&bytes, central_extra), 0x0001);
        assert_eq!(read_u16(&bytes, central_extra + 2), 24);

        assert_eq!(
            read_u32(&bytes, bytes.len() - 98),
            ZIP64_END_OF_CENTRAL_DIRECTORY
        );
        assert_eq!(
            read_u32(&bytes, bytes.len() - 42),
            ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR
        );
        assert_eq!(read_u32(&bytes, bytes.len() - 22), END_OF_CENTRAL_DIRECTORY);
        assert_eq!(
            read_u64(&bytes, bytes.len() - 98 + 40),
            u64::try_from(bytes.len() - central).expect("central directory size")
        );
        assert_eq!(
            read_u32(&bytes, bytes.len() - 22 + 12),
            u32::try_from(bytes.len() - central).expect("legacy central directory size")
        );

        let file = File::open(path).expect("open archive");
        let mut archive = zip::ZipArchive::new(file).expect("decode archive");
        let names = (0..archive.len())
            .map(|index| {
                archive
                    .by_index(index)
                    .expect("archive member")
                    .name()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(names, ["Alpha.pro", "data", "{Zulu}.pro"]);
    }

    #[test]
    fn leaves_utf8_filename_flag_unset_like_native_exports() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("native-utf8.proplaylist");
        let file = File::create(&path).expect("create archive");
        let file = write(
            file,
            vec![Entry::borrowed(
                "Anástasis.pro".to_string(),
                b"presentation",
            )],
        )
        .expect("write archive");
        drop(file);

        let bytes = std::fs::read(path).expect("read archive");
        assert_eq!(read_u16(&bytes, 6), 0, "local-header flags");
        let central = bytes
            .windows(4)
            .position(|window| window == CENTRAL_DIRECTORY_HEADER.to_le_bytes())
            .expect("central directory");
        assert_eq!(read_u16(&bytes, central + 8), 0, "central-header flags");
    }

    fn read_u16(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
    }

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ])
    }

    fn read_u64(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ])
    }
}
