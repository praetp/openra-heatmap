use clap::Clap;
use memmap::Mmap;
use std::fs::File;
use std::io::{Write, Error, ErrorKind};
use std::io::Read;
use byteorder::{ByteOrder, LittleEndian};
use encoding::all::ASCII;
use encoding::all::ISO_8859_1;
use encoding::{Encoding, ByteWriter, EncoderTrap, DecoderTrap};
use num_enum::TryFromPrimitive;
use std::convert::TryFrom;
use serde_json::{Value};
use std::io::copy;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, CONTENT_ENCODING, ACCEPT};

#[derive(Clap)]
#[clap(version = "1.0", author = "Paul P.")]
struct Opts {
    replay_filename: String
}

struct Packet {

}
#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
enum TargetType { Invalid, Actor, Terrain, FrozenActor }

#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
enum OrderType {
    SyncHash = 0x65,
	Disconnect = 0xBF,
	Handshake = 0xFE,
	Fields = 0xFF
}
#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(i16)]
enum OrderFields{
		None = 0x0,
		Target = 0x01,
		ExtraActors = 0x02,
		TargetString = 0x04,
		Queued = 0x08,
		ExtraLocation = 0x10,
		ExtraData = 0x20,
		TargetIsCell = 0x40,
		Subject = 0x80,
		Grouped = 0x100
}

// Given a stream of bytes, extract first number
fn decode_slice(bytes: &[u8], index: &mut usize) -> Result<u32, Error> {
    // Read out an Int32 7 bits at a time.  The high bit
    // of the byte when on means to continue reading more bytes.
    let mut count : u32 = 0;
    let mut shift : u32 = 0;
    let mut b:u16;
    loop {
        // ReadByte handles end of stream cases for us.
        b = bytes[*index] as u16;
        let tmp = (b & 0x7F);
        let tmp2 = (tmp as u32) << shift;
        count |= tmp2 as u32;
        shift += 7;
        *index += 1;

        if (b & 0x80) == 0 {
            return Ok(count)
        }
    }
}

fn construct_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_str("*/*").unwrap());  
    headers
}

fn get_screenshot() -> Result<(), Error> {

    let client = reqwest::blocking::Client::new();
    let response = client                        
                        .get("https://resource.openra.net/map/hash/9ea6b2cb97cd8fca36a5896b1ebec0a3d06381d0")
                        .headers(construct_headers())
                        .send()
                        .unwrap();
    println!("Response: {:?}", response);
    let map_info: Value = response.json().unwrap();
    println!("map_info: {:?}", map_info);
    let object = &map_info[0];
    println!("object: {:?}", object);
    let height = object["height"].as_str();
    println!("height: {:?}", height);
    let id = object["id"].as_u64();
    println!("id: {:?}", id);
    let url = ["https://resource.openra.net/screenshots/", &id.unwrap().to_string()].concat();
    let response = client.get(&url).send().unwrap();
    let mut dest = {
        let fname = response
            .url()
            .path_segments()
            .and_then(|segments| segments.last())
            .and_then(|name| if name.is_empty() { None } else { Some(name) })
            .unwrap_or("tmp.bin");

        println!("file to download: '{}'", fname);
        // let fname = tmp_dir.path().join(fname);
        println!("will be located under: '{:?}'", fname);
        File::create(fname)?
    };
    let content =  response.text().unwrap();
    copy(&mut content.as_bytes(), &mut dest)?;
    Ok(())
}

struct ReplayReader {
    pos: usize,
    map: Mmap
}

impl ReplayReader {
    pub fn new(map: Mmap) -> Self {
        ReplayReader {
            pos: 0,
            map: map
        }
    }

    pub fn read_string(&mut self) -> String {
        let strlength = decode_slice(&self.map, &mut self.pos).unwrap() as usize;

        let rpos = self.pos + strlength;
        let string = ASCII.decode(&self.map[self.pos..rpos], DecoderTrap::Strict).unwrap();
        self.pos = rpos;
        string
    }

    pub fn read_i32(&mut self) -> i32 {
        let integer = LittleEndian::read_i32(&self.map[self.pos..self.pos+4]);
        self.pos += 4;
        integer
    }

    pub fn read_u32(&mut self) -> u32 {
        let integer = LittleEndian::read_u32(&self.map[self.pos..self.pos+4]);
        self.pos += 4;
        integer
    }

    pub fn read_i16(&mut self) -> i16 {
        let integer = LittleEndian::read_i16(&self.map[self.pos..self.pos+2]);
        self.pos += 2;
        integer
    }

    pub fn at_relative_offset(&self, offset: usize) -> u8 {
        self.map[self.pos + offset]
    }

    pub fn read_u8(&mut self) -> u8 {        
        let byte = self.map[self.pos];
        self.pos += 1;
        byte
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }
}

fn main() -> Result<(), Error> {
    println!("Hello, world!");
    let opts: Opts = Opts::parse();
    println!("Reading replay file from : {}", opts.replay_filename);

    let flagsAreShort = false; //look at the version instead

    let file = File::open(opts.replay_filename)?;

    let map = unsafe { Mmap::map(&file)? };
    let mut reader = ReplayReader::new(map);
    loop {
        println!("---------------- index is {}", reader.pos());
        let client = reader.read_i32();
        if (client == -1) {
            break;
        }
        let packetLen = reader.read_i32() as usize;
        let rpos: usize = reader.pos() + packetLen as usize;
        // let packetdata = &map[index..rpos]; //omit client and packetLen
        println!("client is {}", client);
        println!("packetlen is {}", packetLen);
        if packetLen == 5 && reader.at_relative_offset(4) == OrderType::Disconnect as u8 {
            reader.set_pos(reader.pos() + packetLen);
            continue; // disconnect
        } else if (packetLen >= 5 && reader.at_relative_offset(4) == OrderType::SyncHash as u8) {
            reader.set_pos(reader.pos() + packetLen);
            println!("synchash continue");
            continue; // sync
        }

        let frame = reader.read_i32();
        
        while reader.pos() < rpos {
            let ordertypebyte = reader.read_u8();
            let ordertype = OrderType::try_from(ordertypebyte).unwrap();
            println!("order type is {:?}", ordertype);
            match ordertype {
                OrderType::Handshake => {
                   
                        let name = reader.read_string();
                        println!("name {}", name);
                        let targetstring = reader.read_string();
                        println!("targetstring {}", targetstring);
                },
                OrderType::Fields => {
                    let order = reader.read_string();
                    
                    let mut flags = 0;
                    if (flagsAreShort) {
                         flags = reader.read_i16();
                    } else {
                        flags = reader.read_u8() as i16;
                    }
                    println!("order {}, flags {:#02x}", order, flags);

                    
                    if (flags & OrderFields::Subject as i16 > 0) {
                        let subject_id = reader.read_u32();
                    }
                    if (flags & OrderFields::Target as i16 > 0) {
                        let target_type_byte = reader.read_u8();
                        let target_type = TargetType::try_from(target_type_byte).unwrap();
                        println!("target type is {:?}", target_type);
                        match target_type {
                            TargetType::Actor => {
                                let actor_id = reader.read_u32();

                            },
                            TargetType::FrozenActor => {
                                let player_actor_id =  reader.read_u32();
                                let frozen_actor_id =  reader.read_u32();
                            },
                            TargetType::Terrain => {
                                   if (flags & OrderFields::TargetIsCell as i16 > 0) {
                                        let cell =  reader.read_u32();
                                        let x = cell >> 20 as i16;
                                        let y = ((cell >> 8) & 0xFFF) as i16;
                                        let z = cell as u8;
                                        println!("to {},{},{}", x, y, z);
                                        let subcell = reader.read_u8();
                                   } else {
                                        let x =  reader.read_u32();
                                        let y =  reader.read_u32();
                                        let z = reader.read_u32();
                                   }
                            },
                            _ => {
                                panic!("wtf");
                            }

                        }
                    }
                    if (flags & OrderFields::TargetString as i16 > 0) {
                        let target_string = reader.read_string();
                        println!("target_string {}", target_string);
                    }
                    if (flags & OrderFields::ExtraActors as i16 > 0) {
                        let count =  reader.read_u32();
                        let mut vec = Vec::new();
                        for i in 0..count {
                            let tmp =  reader.read_u32();
                            vec.push(tmp)
                        }
                    }
                    if (flags & OrderFields::ExtraLocation as i16 > 0) {
                        let pos =  reader.read_i32();                        
                    }
                    if (flags & OrderFields::ExtraData as i16 > 0) {
                        let extradata =  reader.read_u32();
                    }
                    if (flags & OrderFields::Grouped as i16 > 0) {
                        let count =  reader.read_i32();
                        let mut vec = Vec::new();
                        for i in 0..count {
                            let tmp =  reader.read_u32();
                            vec.push(tmp)
                        }
                    }

                    

                    // return Ok(());
                },
                OrderType::SyncHash => {
                    //noop
                    println!("sync hash");
                },
                _ => {
                    println!("ordertype {:?} not supported", ordertype);
                }

            }
        }
        
        //return Ok(());
    }
    println!("now read in the metadata");
  
    Ok(())
}
