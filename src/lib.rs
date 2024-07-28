// https://pkware.cachefly.net/webdocs/APPNOTE/APPNOTE-6.3.9.TXT

extern crate byteorder;

use byteorder::{LittleEndian, ReadBytesExt};
use flate2::read::DeflateDecoder;
use log::{error, info};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::{io, mem};

pub struct UnZip {
    archive: File,
    destination: PathBuf,
}
impl UnZip {
    pub fn try_from<T: Into<PathBuf>>(name: T, destination: T) -> io::Result<Self> {
        let name = name.into();
        let destination = destination.into();

        info!("UnZip{} -> {}", name.display(), destination.display());
        let archive = File::open(name).inspect_err(|e| error!("{e}"))?;
        let destination = destination;
        Ok(Self {
            archive,
            destination,
        })
    }

    pub fn file<T: Into<String>>(&mut self, file: T) -> Result<(), Box<dyn std::error::Error>> {
        let file = file.into();
        info!("UnZip.file <- {}", file);
        let res = self.file_impl(file).inspect_err(|e| error!("{e}"))?;
        info!("UnZip.file -> {}", res);
        Ok(())
    }

    fn file_impl(&mut self, file: String) -> Result<String, Box<dyn std::error::Error>> {
        let mut total_files: u64 = 0;
        EndOfCentralDirectoryRecord::search(&mut self.archive)?;
        loop {
            let signature = self.archive.read_u32::<LittleEndian>()?;
            match signature {
                LocalFileHeader::SIGNATURE => {
                    let lfh = LocalFileHeader::from_reader(&mut self.archive)?;
                    if file == lfh.file_name {
                        let destination = self.destination.join(file);
                        let mut ofile = File::create(&destination)?;
                        lfh.decompress_to(&mut self.archive, &mut ofile)?;
                        return Ok(destination.display().to_string());
                    } else {
                        return Err(format!("Unexpected LFH").into());
                    }
                }
                CentralDirectoryHeader::SIGNATURE => {
                    let cdr = CentralDirectoryHeader::from_reader(&mut self.archive)?;
                    total_files -= 1;

                    if file == cdr.file_name {
                        cdr.seek_to_local_file_header(&mut self.archive)?;
                    } else if 0 == total_files {
                        break;
                    }
                }
                Zip64EndOfCentralDirectoryLocator::SIGNATURE => {
                    let locator =
                        Zip64EndOfCentralDirectoryLocator::from_reader(&mut self.archive)?;
                    locator.seek_to_zip64_end_of_central_directory_record(&mut self.archive)?;
                }
                Zip64EndOfCentralDirectoryRecord::SIGNATURE => {
                    let eocdr = Zip64EndOfCentralDirectoryRecord::from_reader(&mut self.archive)?;
                    total_files = eocdr.total_number_of_entries_in_the_central_directory;
                    eocdr.seek_to_start_of_central_directory(&mut self.archive)?;
                }
                EndOfCentralDirectoryRecord::SIGNATURE => {
                    let eocdr = EndOfCentralDirectoryRecord::from_reader(&mut self.archive)?;
                    if eocdr.is_zip64() {
                        eocdr.seek_to_zip64_eocdr_locator(&mut self.archive)?;
                    } else {
                        total_files = eocdr.total_number_of_entries_in_the_central_directory as u64;
                        eocdr.seek_to_start_of_central_directory(&mut self.archive)?;
                    }
                }
                _ => {
                    return Err(format!("Unexpected signature: 0x{:05X}", signature).into());
                }
            }
        }
        return Err(format!("The {file} is not found in {:?}", self.archive).into());
    }
}

pub fn read_signature<R: Read>(mut reader: R) -> io::Result<u32> {
    reader.read_u32::<LittleEndian>()
}

#[derive(Debug)]
pub struct LocalFileHeader {
    pub version: u16,
    pub flags: u16,
    pub compression_method: u16,
    pub last_mod_time: u16,
    pub last_mod_date: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub file_name_length: u16,
    pub extra_field_length: u16,
    pub file_name: String,
}

impl LocalFileHeader {
    pub const SIGNATURE: u32 = 0x04034b50;

    pub fn from_reader<R: Read + Seek>(mut reader: R) -> io::Result<Self> {
        let version = reader.read_u16::<LittleEndian>()?;
        let flags = reader.read_u16::<LittleEndian>()?;
        let compression_method = reader.read_u16::<LittleEndian>()?;
        let last_mod_time = reader.read_u16::<LittleEndian>()?;
        let last_mod_date = reader.read_u16::<LittleEndian>()?;
        let crc32 = reader.read_u32::<LittleEndian>()?;
        let mut compressed_size = reader.read_u32::<LittleEndian>()? as u64;
        let mut uncompressed_size = reader.read_u32::<LittleEndian>()? as u64;
        let file_name_length = reader.read_u16::<LittleEndian>()?;
        let mut extra_field_length = reader.read_u16::<LittleEndian>()?;
        let mut file_name = vec![0; file_name_length as usize];
        reader.read_exact(&mut file_name)?;
        let file_name: String = String::from_utf8_lossy(&file_name).to_string();

        while extra_field_length > 0 {
            let header = Header::from_reader(&mut reader)?;
            extra_field_length -= mem::size_of_val(&header.id) as u16;
            extra_field_length -= mem::size_of_val(&header.size) as u16;
            extra_field_length -= header.size;
            if header.id == 0x0001 {
                if 0xFFFFFFFF == uncompressed_size {
                    uncompressed_size = reader.read_u64::<LittleEndian>()?;
                }
                if 0xFFFFFFFF == compressed_size {
                    compressed_size = reader.read_u64::<LittleEndian>()?;
                }
            } else {
                reader.seek(SeekFrom::Current(header.size as i64))?;
            }
        }

        Ok(LocalFileHeader {
            version,
            flags,
            compression_method,
            last_mod_time,
            last_mod_date,
            crc32,
            compressed_size,
            uncompressed_size,
            file_name_length,
            extra_field_length,
            file_name,
        })
    }

    pub fn skip_compressed<R: Read + Seek>(&self, mut reader: R) -> io::Result<u64> {
        reader.seek(SeekFrom::Current(self.compressed_size as i64))
    }

    pub fn load_compressed<R: Read + Seek>(&self, mut reader: R) -> io::Result<Vec<u8>> {
        let mut compressed = vec![0; self.compressed_size as usize];
        reader.read_exact(&mut compressed)?;
        Ok(compressed)
    }

    pub fn decompress_to<R: Read + Seek, W: Write>(
        &self,
        mut reader: R,
        mut writer: W,
    ) -> io::Result<()> {
        let compressed = self.load_compressed(&mut reader)?;
        Self::write(&compressed, &mut writer)
    }

    fn write<W: Write>(compressed: &[u8], mut writer: W) -> io::Result<()> {
        let mut decoder = DeflateDecoder::new(compressed);
        let mut decompressed = [0; 1024 * 1024];
        while let Ok(bytes) = decoder.read(&mut decompressed) {
            if bytes > 0 {
                writer.write_all(&decompressed[..bytes])?;
            } else {
                break;
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct CentralDirectoryHeader {
    pub version_made_by: u16,
    pub version_needed: u16,
    pub flags: u16,
    pub compression_method: u16,
    pub last_mod_time: u16,
    pub last_mod_date: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub file_name_length: u16,
    pub extra_field_length: u16,
    pub file_comment_length: u16,
    pub disk_number_start: u32,
    pub internal_file_attributes: u16,
    pub external_file_attributes: u32,
    pub local_header_offset: u64,
    pub file_name: String,
}

impl CentralDirectoryHeader {
    pub const SIGNATURE: u32 = 0x02014b50;

    pub fn from_reader<R: Read + Seek>(mut reader: R) -> io::Result<Self> {
        let version_made_by = reader.read_u16::<LittleEndian>()?;
        let version_needed = reader.read_u16::<LittleEndian>()?;
        let flags = reader.read_u16::<LittleEndian>()?;
        let compression_method = reader.read_u16::<LittleEndian>()?;
        let last_mod_time = reader.read_u16::<LittleEndian>()?;
        let last_mod_date = reader.read_u16::<LittleEndian>()?;
        let crc32 = reader.read_u32::<LittleEndian>()?;
        let mut compressed_size = reader.read_u32::<LittleEndian>()? as u64;
        let mut uncompressed_size = reader.read_u32::<LittleEndian>()? as u64;
        let file_name_length = reader.read_u16::<LittleEndian>()?;
        let mut extra_field_length = reader.read_u16::<LittleEndian>()?;
        let file_comment_length = reader.read_u16::<LittleEndian>()?;
        let mut disk_number_start = reader.read_u16::<LittleEndian>()? as u32;
        let internal_file_attributes = reader.read_u16::<LittleEndian>()?;
        let external_file_attributes = reader.read_u32::<LittleEndian>()?;
        let mut local_header_offset = reader.read_u32::<LittleEndian>()? as u64;

        let file_name = if file_name_length > 0 {
            let mut file_name = vec![0; file_name_length as usize];
            reader.read_exact(&mut file_name)?;
            String::from_utf8_lossy(&file_name).to_string()
        } else {
            String::new()
        };

        while extra_field_length > 0 {
            let header = Header::from_reader(&mut reader)?;
            extra_field_length -= mem::size_of_val(&header.id) as u16;
            extra_field_length -= mem::size_of_val(&header.size) as u16;
            extra_field_length -= header.size;
            if header.id == 0x0001 {
                if 0xFFFFFFFF == uncompressed_size {
                    uncompressed_size = reader.read_u64::<LittleEndian>()?;
                }
                if 0xFFFFFFFF == compressed_size {
                    compressed_size = reader.read_u64::<LittleEndian>()?;
                }
                if 0xFFFFFFFF == local_header_offset {
                    local_header_offset = reader.read_u64::<LittleEndian>()?;
                }
                if 0xFFFF == disk_number_start {
                    disk_number_start = reader.read_u32::<LittleEndian>()?;
                }
            } else {
                reader.seek(SeekFrom::Current(header.size as i64))?;
            }
        }
        if file_comment_length > 0 {
            reader.seek(SeekFrom::Current(file_comment_length as i64))?;
        }

        Ok(CentralDirectoryHeader {
            version_made_by,
            version_needed,
            flags,
            compression_method,
            last_mod_time,
            last_mod_date,
            crc32,
            compressed_size,
            uncompressed_size,
            file_name_length,
            extra_field_length,
            file_comment_length,
            disk_number_start,
            internal_file_attributes,
            external_file_attributes,
            local_header_offset,
            file_name,
        })
    }

    pub fn seek_to_local_file_header<R: Read + Seek>(&self, mut reader: R) -> io::Result<u64> {
        reader.seek(SeekFrom::Start(self.local_header_offset))
    }
}

#[derive(Debug)]
pub struct EndOfCentralDirectoryRecord {
    pub number_of_this_disk: u16,
    pub number_of_the_disk_with_the_start_of_the_central_directory: u16,
    pub total_number_of_entries_in_the_central_directory_on_this_disk: u16,
    pub total_number_of_entries_in_the_central_directory: u16,
    pub size_of_the_central_directory: u32,
    pub offset_of_start_of_central_directory_with_respect_to_the_starting_disk_number: u32,
    pub comment_length: u16,
    pub ofset_in_file: u64,
}

impl EndOfCentralDirectoryRecord {
    pub const SIGNATURE: u32 = 0x06054b50;
    pub fn from_reader<R: Read + Seek>(mut reader: R) -> io::Result<Self> {
        let ofset_in_file =
            reader.seek(SeekFrom::Current(0))? - mem::size_of_val(&Self::SIGNATURE) as u64;
        let number_of_this_disk = reader.read_u16::<LittleEndian>()?;
        let number_of_the_disk_with_the_start_of_the_central_directory =
            reader.read_u16::<LittleEndian>()?;
        let total_number_of_entries_in_the_central_directory_on_this_disk =
            reader.read_u16::<LittleEndian>()?;
        let total_number_of_entries_in_the_central_directory = reader.read_u16::<LittleEndian>()?;
        let size_of_the_central_directory = reader.read_u32::<LittleEndian>()?;
        let offset_of_start_of_central_directory_with_respect_to_the_starting_disk_number =
            reader.read_u32::<LittleEndian>()?;
        let comment_length = reader.read_u16::<LittleEndian>()?;

        Ok(EndOfCentralDirectoryRecord {
            number_of_this_disk,
            number_of_the_disk_with_the_start_of_the_central_directory,
            total_number_of_entries_in_the_central_directory_on_this_disk,
            total_number_of_entries_in_the_central_directory,
            size_of_the_central_directory,
            offset_of_start_of_central_directory_with_respect_to_the_starting_disk_number,
            comment_length,
            ofset_in_file,
        })
    }
    pub fn is_zip64(&self) -> bool {
        self.number_of_the_disk_with_the_start_of_the_central_directory == 0xFFFF
            || self.total_number_of_entries_in_the_central_directory_on_this_disk == 0xFFFF
            || self.total_number_of_entries_in_the_central_directory == 0xFFFF
            || self.size_of_the_central_directory == 0xFFFFFFFF
            || self.offset_of_start_of_central_directory_with_respect_to_the_starting_disk_number
                == 0xFFFFFFFF
    }
    pub fn seek_to_zip64_eocdr_locator<R: Read + Seek>(&self, mut reader: R) -> io::Result<u64> {
        reader.seek(SeekFrom::Start(
            self.ofset_in_file
                - mem::size_of::<Zip64EndOfCentralDirectoryLocator>() as u64
                - mem::size_of_val(&Zip64EndOfCentralDirectoryLocator::SIGNATURE) as u64,
        ))
    }
    pub fn seek_to_start_of_central_directory<R: Read + Seek>(
        &self,
        mut reader: R,
    ) -> io::Result<u64> {
        reader.seek(SeekFrom::Start(
            self.offset_of_start_of_central_directory_with_respect_to_the_starting_disk_number
                as u64,
        ))
    }

    pub fn search<R: Read + Seek>(mut reader: R) -> io::Result<u64> {
        // TODO implement extended search, if comment exists
        let offset: i64 = -22;
        reader.seek(SeekFrom::End(offset))
    }
}

#[derive(Debug)]
pub struct Zip64EndOfCentralDirectoryRecord {
    pub size_of_zip64_end_of_central: u64,
    pub version_made_by: u16,
    pub version_needed: u16,
    pub number_of_this_disk: u32,
    pub number_of_the_disk_with_the_start_of_the_central_directory: u32,
    pub total_number_of_entries_in_the_central_directory_on_this_disk: u64,
    pub total_number_of_entries_in_the_central_directory: u64,
    pub size_of_the_central_directory: u64,
    pub offset_of_start_of_central_directory_with_respect_to_the_starting_disk_number: u64,
}

impl Zip64EndOfCentralDirectoryRecord {
    pub const SIGNATURE: u32 = 0x06064b50;
    pub fn from_reader<R: Read>(mut reader: R) -> io::Result<Self> {
        Ok(Self {
            size_of_zip64_end_of_central: reader.read_u64::<LittleEndian>()?,
            version_made_by: reader.read_u16::<LittleEndian>()?,
            version_needed: reader.read_u16::<LittleEndian>()?,
            number_of_this_disk: reader.read_u32::<LittleEndian>()?,
            number_of_the_disk_with_the_start_of_the_central_directory: reader
                .read_u32::<LittleEndian>()?,
            total_number_of_entries_in_the_central_directory_on_this_disk: reader
                .read_u64::<LittleEndian>()?,
            total_number_of_entries_in_the_central_directory: reader.read_u64::<LittleEndian>()?,
            size_of_the_central_directory: reader.read_u64::<LittleEndian>()?,
            offset_of_start_of_central_directory_with_respect_to_the_starting_disk_number:
                reader.read_u64::<LittleEndian>()?,
        })
    }
    pub fn seek_to_start_of_central_directory<R: Read + Seek>(
        &self,
        mut reader: R,
    ) -> io::Result<u64> {
        reader.seek(SeekFrom::Start(
            self.offset_of_start_of_central_directory_with_respect_to_the_starting_disk_number,
        ))
    }
}

#[derive(Debug)]
pub struct Zip64EndOfCentralDirectoryLocator {
    /*pub  signature: u32 */
    pub number_of_the_disk_with_the_start_of_the_zip64_end_of_central_directory: u32,
    pub relative_offset_of_the_zip64_end_of_central_directory_record: u64,
    pub total_number_of_disks: u32,
}
impl Zip64EndOfCentralDirectoryLocator {
    pub const SIGNATURE: u32 = 0x07064b50;
    pub const SIZE: u64 = 20;
    pub fn from_reader<R: Read>(mut reader: R) -> io::Result<Self> {
        Ok(Self {
            number_of_the_disk_with_the_start_of_the_zip64_end_of_central_directory: reader
                .read_u32::<LittleEndian>(
            )?,
            relative_offset_of_the_zip64_end_of_central_directory_record: reader
                .read_u64::<LittleEndian>()?,
            total_number_of_disks: reader.read_u32::<LittleEndian>()?,
        })
    }
    pub fn seek_to_zip64_end_of_central_directory_record<R: Read + Seek>(
        &self,
        mut reader: R,
    ) -> io::Result<u64> {
        reader.seek(SeekFrom::Start(
            self.relative_offset_of_the_zip64_end_of_central_directory_record,
        ))
    }
}

#[derive(Debug)]
struct Header {
    pub id: u16,
    pub size: u16,
}
impl Header {
    pub fn from_reader<R: Read + Seek>(mut reader: R) -> io::Result<Self> {
        let id = reader.read_u16::<LittleEndian>()?;
        let size = reader.read_u16::<LittleEndian>()?;
        Ok(Self { id, size })
    }
}
