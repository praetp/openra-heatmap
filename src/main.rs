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

fn readString(packetdata : &[u8], index: &mut usize) -> String {
    
    let strlength = decode_slice(packetdata, index).unwrap() as usize;

    let rpos = *index + strlength;
    let string = ASCII.decode(&packetdata[*index..rpos], DecoderTrap::Strict).unwrap();
    *index = rpos;
    return string;
}

// fn read_i16(packetdata : &[u8], index: &mut usize) -> i16 {

// }

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

fn main() -> Result<(), Error> {
    println!("Hello, world!");
    let opts: Opts = Opts::parse();
    println!("Reading replay file from : {}", opts.replay_filename);

    let flagsAreShort = false; //look at the version instead

    let file = File::open(opts.replay_filename)?;

    let map = unsafe { Mmap::map(&file)? };
    let mut index = 0 as usize;
    loop {
        println!("---------------- index is {}", index);
        let client = LittleEndian::read_i32(&map[index..index+4]);
        if (client == -1) {
            break;
        }
        index += 4;
        let packetLen = LittleEndian::read_i32(&map[index..index+4]) as usize;
        index += 4;
        let rpos: usize = index + packetLen as usize;
        // let packetdata = &map[index..rpos]; //omit client and packetLen
        println!("client is {}", client);
        println!("packetlen is {}", packetLen);
        if packetLen == 5 && map[index+4] == OrderType::Disconnect as u8 {
            index += packetLen;
            continue; // disconnect
        } else if (packetLen >= 5 && map[index+4] == OrderType::SyncHash as u8) {
            index += packetLen;
            println!("synchash continue");
            continue; // sync
        }

        index += 4 as usize; //we don't care about the frame number
        
        while index < rpos {
            let ordertypebyte = map[index];
            let ordertype = OrderType::try_from(ordertypebyte).unwrap();
            println!("order type is {:?}", ordertype);
            index += 1;
            match ordertype {
                OrderType::Handshake => {
                   
                        let name = readString(&map, &mut index);
                        println!("name {}", name);
                        let targetstring = readString(&map, &mut index);
                        println!("targetstring {}", targetstring);
                },
                OrderType::Fields => {
                    let order = readString(&map, &mut index);
                    
                    let mut flags = 0;
                    if (flagsAreShort) {
                         flags = LittleEndian::read_i16(&map[index..index+2]);
                         index += 2;
                    } else {
                        flags = map[index] as i16;
                        index += 1;
                    }
                    println!("order {}, flags {:#02x}", order, flags);

                    
                    if (flags & OrderFields::Subject as i16 > 0) {
                        let subject_id = LittleEndian::read_u32(&map[index..index+4]);
                        index += 4;
                    }
                    if (flags & OrderFields::Target as i16 > 0) {
                        let target_type_byte = map[index];
                        index += 1;
                        let target_type = TargetType::try_from(target_type_byte).unwrap();
                        println!("target type is {:?}", target_type);
                        match target_type {
                            TargetType::Actor => {
                                let actor_id = LittleEndian::read_u32(&map[index..index+4]);
                                index += 4;

                            },
                            TargetType::FrozenActor => {
                                let player_actor_id =  LittleEndian::read_u32(&map[index..index+4]);
                                index += 4;
                                let frozen_actor_id =  LittleEndian::read_u32(&map[index..index+4]);
                                index += 4;
                            },
                            TargetType::Terrain => {
                                   if (flags & OrderFields::TargetIsCell as i16 > 0) {
                                        let cell =  LittleEndian::read_u32(&map[index..index+4]);
                                        index += 4; 
                                        let subcell = map[index];
                                        index += 1;
                                   } else {
                                        let x =  LittleEndian::read_u32(&map[index..index+4]);
                                        index += 4; 
                                        let y =  LittleEndian::read_u32(&map[index..index+4]);
                                        index += 4; 
                                        let z =  LittleEndian::read_u32(&map[index..index+4]);
                                        index += 4; 
                                   }
                            },
                            _ => {
                                panic!("wtf");
                            }

                        }
                    }
                    if (flags & OrderFields::TargetString as i16 > 0) {
                        let target_string = readString(&map, &mut index);
                        println!("target_string {}", target_string);
                    }
                    if (flags & OrderFields::ExtraActors as i16 > 0) {
                        let count =  LittleEndian::read_u32(&map[index..index+4]);
                        index += 4; 
                        let mut vec = Vec::new();
                        for i in 0..count {
                            let tmp =  LittleEndian::read_u32(&map[index..index+4]);
                            index += 4; 
                            vec.push(tmp)
                        }
                    }
                    if (flags & OrderFields::ExtraLocation as i16 > 0) {
                        let pos =  LittleEndian::read_i32(&map[index..index+4]);
                            index += 4; 
                    }
                    if (flags & OrderFields::ExtraData as i16 > 0) {
                        let extradata =  LittleEndian::read_u32(&map[index..index+4]);
                        index += 4; 
                    }
                    if (flags & OrderFields::Grouped as i16 > 0) {
                        let count =  LittleEndian::read_i32(&map[index..index+4]);
                        index += 4; 
                        let mut vec = Vec::new();
                        for i in 0..count {
                            let tmp =  LittleEndian::read_u32(&map[index..index+4]);
                            index += 4; 
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
