use clap::Clap;
use memmap::Mmap;
use std::fs::File;
use std::io::{Write, Error, ErrorKind};
use byteorder::{ByteOrder, LittleEndian};
use encoding::all::UTF_8;
use encoding::{Encoding, ByteWriter, EncoderTrap, DecoderTrap};
use num_enum::TryFromPrimitive;
use std::convert::TryFrom;
use serde_json::{Value};
use std::io::copy;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, CONTENT_ENCODING, ACCEPT};
use std::collections::HashMap;
use image::GenericImageView;
use image::DynamicImage;
use image::{Pixel, Pixels};
use image::{ImageBuffer, Rgb, Rgba};
use imageproc::drawing::{Canvas, Blend};
use rusttype::Font;
use rusttype::Scale;
use std::path::Path;
use regex::Regex;
// use hyper::header::{Headers, ContentDisposition, DispositionType, DispositionParam, Charset};

#[macro_use] extern crate lazy_static;
use bytes::Buf;

#[derive(Clap)]
#[clap(version = "1.0", author = "Paul P.")]
struct Opts {
    replay_filename: String
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
        let tmp = b & 0x7F;
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

//if https://github.com/OpenRA/OpenRA-Resources/pull/365 get submitted we don't have to do this anymore
fn find_screenshot_id(map_id : u32) -> Option<u32> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"/screenshots/(\d+)/").unwrap();
    }
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/{}", "https://resource.openra.net/maps", map_id);
    let response = client
                        .get(&url)
                        .headers(construct_headers())
                        .send()
                        .unwrap();
    println!("response {:?}", response);
    let buffer = response.text().expect("cannot read response");
    println!("buffer {}", buffer);

    
    if RE.is_match(&buffer) {
        let caps = RE.captures(&buffer).unwrap();
        return Some(caps.get(1).map_or(0, |m| m.as_str().parse::<u32>().expect("Cannot parse screenshot id")));
    } else {
        return None;
    }
}

struct MapInfo {
    id: u32,
    width: u16,
    height: u16
}

fn get_map_info(hash: &str) -> Result<MapInfo, Error> {

    let client = reqwest::blocking::Client::new();
    let url = format!("{}/{}", "https://resource.openra.net/map/hash/", hash);
    let response = client                        
                        .get(&url)
                        .headers(construct_headers())
                        .send()
                        .unwrap();
    // println!("Response: {:?}", response);
    let map_info: Value = response.json().unwrap();
    // println!("map_info: {:?}", map_info);
    let object = &map_info[0];
    let id = object["id"].as_u64().unwrap() as u32;
    // println!("object: {:?}", object);
    let height = object["height"].as_str().unwrap().parse::<u16>().expect("cannot parse height");
    // println!("height: {:?}", height);
    let width = object["width"].as_str().unwrap().parse::<u16>().expect("cannot parse width");
    // println!("width: {:?}", width);

    Ok(MapInfo {
        id,
        width,
        height
    })   
}

fn read_screenshot(path: &str) -> DynamicImage {
    // Use the open function to load an image from a Path.
    // `open` returns a `DynamicImage` on success.
    let img = image::open(path).expect("Could not open screenshot file");

    // The dimensions method returns the images width and height.
    println!("dimensions {:?}",GenericImageView::dimensions(&img));

    // The color method returns the image's `ColorType`.
    println!("{:?}", img.color());

    img
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

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn read_string(&mut self) -> String {
        let strlength = decode_slice(&self.map, &mut self.pos).unwrap() as usize;

        let rpos = self.pos + strlength;
        let string = UTF_8.decode(&self.map[self.pos..rpos], DecoderTrap::Replace).unwrap();
        self.pos = rpos;
        string
    }

    pub fn read_string_with_length(&mut self, strlength: usize) -> String {

        let rpos = self.pos + strlength;
        let string = UTF_8.decode(&self.map[self.pos..rpos], DecoderTrap::Strict).unwrap();
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

struct Player {
    client_id: i32,
    name: String,
    color: Rgba<u8>
}

struct GameInformation {
    version: String,
    map_uid : String,
    players: HashMap<i32, Player>
}

fn download_screenshot(screenshot_id : u32) -> String {
    let client = reqwest::blocking::Client::new(); //TODO SHARE CLIENTS
   
    let url = format!("{}/{}", "https://resource.openra.net/screenshots", screenshot_id);
    let response = client.get(&url).send();
    println!("Response {:?}", response);
    let response = response.expect("response problem");
    let header = response.headers().get(reqwest::header::CONTENT_DISPOSITION).expect("Expected content-disposition");
    let disp = header.to_str().expect("header was not a string");

    let fname = format!("{}.png", screenshot_id);
    let mut dest = {
        
        println!("file to download: '{}'", fname);
        // let fname = tmp_dir.path().join(fname);
        println!("will be located under: '{:?}'", fname);
        File::create(fname.clone()).expect("Could not create file")
    };
    let content =  response.bytes().expect("Could not get bytes");
    dest.write_all(content.bytes()).expect("Could not write");
    fname
}

fn get_game_information(reader : &mut ReplayReader) -> GameInformation {

    fn save_player(players: &mut HashMap<i32, Player>, client_id: Option<i32>, name: Option<&str>, color: Option<&str>) -> () {
        let client_id_raw = client_id.expect("client id must be present");
        let color =  i32::from_str_radix(color.expect("color must be present"), 16).expect("could not parse color");
        let color_vector = [(color >> 16) as u8, (color >> 8) as u8, color as u8, 255];
        players.insert(client_id_raw, Player {
            client_id: client_id_raw,
            name: name.expect("name must be present").to_string(),
            color: Rgba(color_vector)
        });
    }
 
    print!("Reading in metadata...");
    let total_len = reader.len();
    reader.set_pos(total_len - 8);
    let metadata_len = reader.read_i32() as usize;
    let marker = reader.read_i32();
    if (marker != -2) {
        panic!("End marker NOK")
    }
    reader.set_pos(total_len - (8 + metadata_len + 8));
    let start_marker = reader.read_i32();
    if (start_marker != -1) {
        panic!("Expected start marker");
    }
    let metadata_version = reader.read_i32();
    println!("version is {}", metadata_version);
    let strlen = reader.read_i32() as usize;
    /* this string is encoded differently than all other strings.. */
    let metadata = reader.read_string_with_length(strlen);
    println!("metadata {}", metadata);
    let lines: Vec<_> = metadata.lines().collect();
    let mut client_id:Option<i32> = None;
    let mut name: Option<&str> = None;
    let mut color: Option<&str> = None;
    let mut players: HashMap<i32, Player> = HashMap::new();
    let mut map_uid = None;
    let mut version = None;
    for l in lines {
        let trimmed = l.trim();
        if trimmed.starts_with("Player@") {
            if client_id.is_some() {
                save_player(&mut players, client_id, name, color);

                client_id = None;
                name = None;
                color = None;
            }
           

        } else if trimmed.starts_with("ClientIndex:") {
            client_id = Some(get_rhs(trimmed).parse().unwrap());
        } else if trimmed.starts_with("Name:") {
            name = Some(get_rhs(trimmed));
        } else if trimmed.starts_with("Color:") {
            color = Some(get_rhs(trimmed));
        } else if trimmed.starts_with("MapUid:") {
             map_uid = Some(get_rhs(trimmed));
            println!("mapuid is {}", map_uid.unwrap());
        }else if trimmed.starts_with("Version:") {
            version = Some(get_rhs(trimmed));
           println!("version is {}", version.unwrap());
       }
    }
    save_player(&mut players, client_id, name, color);
    reader.set_pos(0); //reset to beginning
    GameInformation {
        version: version.expect("game version must be present").to_string(),
        map_uid : map_uid.expect("mapuid must be present").to_string(),
        players
    }
}

fn main() -> Result<(), Error> {
    const RED : Rgba<u8> = Rgba([255, 0 , 0, 255]);
    const GREEN : Rgba<u8>= Rgba([0, 255 , 0, 255]);
    const BLUE : Rgba<u8> = Rgba([0, 0 , 255, 255]);
    let last_release_with_byte_for_flags: String = String::from("release-20200503"); //why cannot make this a constant ?
    let opts: Opts = Opts::parse();
    println!("Reading replay file from : {}", opts.replay_filename);


    let file = File::open(opts.replay_filename)?;
    let map = unsafe { Mmap::map(&file)? };
    let mut reader = ReplayReader::new(map);
    let game_information = get_game_information(&mut reader);
    let flags_are_short : bool = game_information.version == "{{DEV_VERSION}}" 
                                || game_information.version > last_release_with_byte_for_flags;

    let map_info = get_map_info(&game_information.map_uid).expect("Could not get map info");
    
    let screenshot_id = find_screenshot_id(map_info.id);
    println!("screenshot id {:#?}", screenshot_id);
    if screenshot_id.is_none() {
        return Err(Error::new(ErrorKind::Other, "Unfortunately, no screenshot is available for download.. Maybe you could upload one ?"));
    }
    let screenshot = format!("{}.png", screenshot_id.unwrap());
    if !Path::new(&screenshot).exists() {
        println!("Screenshot not yet present - need to download it");
        download_screenshot(screenshot_id.unwrap());
    } else {
        println!("Screenshot already there");
    }
    let mut image = read_screenshot(&screenshot);
    let (screenshot_dim_x, screenshot_dim_y) = GenericImageView::dimensions(&image);
    let x_ratio = screenshot_dim_x as f32 / map_info.width as f32;
    let y_ratio = screenshot_dim_y as f32 / map_info.height as f32;

    println!("Reading in frames..");
    loop {
        let client = reader.read_i32();
        
        if client == -1 {
            break;
        }        
       
        let packet_len = reader.read_i32() as usize;
        let rpos: usize = reader.pos() + packet_len as usize;
     
        if packet_len == 5 && reader.at_relative_offset(4) == OrderType::Disconnect as u8 {
            reader.set_pos(reader.pos() + packet_len);
            continue; // disconnect
        } else if packet_len >= 5 && reader.at_relative_offset(4) == OrderType::SyncHash as u8 {
            reader.set_pos(reader.pos() + packet_len);
            continue; // sync
        }

        let frame = reader.read_i32();
        while reader.pos() < rpos {
            let ordertypebyte = reader.read_u8();
            let ordertype = OrderType::try_from(ordertypebyte).unwrap();
            match ordertype {
                OrderType::Handshake => {
                   
                        let name = reader.read_string();
                        let targetstring = reader.read_string();
                },
                OrderType::Fields => {
                    let order = reader.read_string();
                    
                    let flags;
                    if flags_are_short {
                         flags = reader.read_i16();
                    } else {
                        flags = reader.read_u8() as i16;
                    }
                    // println!("order {}, flags {:#02x}", order, flags);

                    
                    if flags & OrderFields::Subject as i16 > 0 {
                        let subject_id = reader.read_u32();
                    }
                    if flags & OrderFields::Target as i16 > 0 {
                        let target_type_byte = reader.read_u8();
                        let target_type = TargetType::try_from(target_type_byte).unwrap();
                        // println!("target type is {:?}", target_type);
                        match target_type {
                            TargetType::Actor => {
                                let actor_id = reader.read_u32();

                            },
                            TargetType::FrozenActor => {
                                let player_actor_id =  reader.read_u32();
                                let frozen_actor_id =  reader.read_u32();
                            },
                            TargetType::Terrain => {
                                   if flags & OrderFields::TargetIsCell as i16 > 0 {
                                        let cell =  reader.read_u32();
                                        let world_x = (cell >> 20) as i16;
                                        let world_y = ((cell >> 8) & 0xFFF) as i16;
                                        let world_z = cell as u8;
                                        let subcell = reader.read_u8();
                                       
                                        let player = game_information.players.get(&client).expect("unknown client-id");
                                        
                                        
                                        //improve https://users.rust-lang.org/t/how-do-i-copy-contents-of-image-into-an-image-buffer/33206/5

                                        for xd in -4..5 {
                                            for yd in -4..5 {
                                                let x = (x_ratio / 2.0 + world_x as f32 * x_ratio) as i16 + xd;
                                                let y = (y_ratio / 2.0 + world_y as f32 * y_ratio) as i16 + yd;
                                                let pixel = if 2 < i16::abs(xd) || 2 < i16::abs(yd) {
                                                    if order == "AttackMove" || order == "AssaultMove" || order == "ForceAttack" || order == "Move" || order == "PlaceBuilding" {
                                                        Some(player.color)
                                                    } else {
                                                        None
                                                    }    
                                                } else {
                                                    if order == "AttackMove" || order == "AssaultMove" || order == "ForceAttack" {                                                        
                                                        Some(RED)
                                                    } else if order == "Move" {
                                                        Some(GREEN)
                                                    }  else if order == "PlaceBuilding" {
                                                        Some(BLUE)
                                                    } else if order == "SetRallyPoint" || order == "Harvest" || order == "BeginMinefield" || order == "PlaceMinefield" {
                                                        // panic!("order {}", order);
                                                        None
                                                    } else {
                                                        None
                                                    }
                                                                                                    
                                                };
                                                if pixel.is_some() {
                                                    image.as_mut_rgba8().unwrap().put_pixel(x as u32, y as u32, pixel.unwrap());
                                                }
                                            }
                                        }
                                        
                                        
                                   } else {
                                        let x =  reader.read_u32() as i16;
                                        let y =  reader.read_u32() as i16;
                                        let z = reader.read_u32() as u8;
                                        
                                   }
                            },
                            TargetType::Invalid => {}
                        }
                    }
                    if flags & OrderFields::TargetString as i16 > 0 {
                        let target_string = reader.read_string();
                        // println!("target_string {}", target_string);                        
                    }
                    if flags & OrderFields::ExtraActors as i16 > 0 {
                        let count =  reader.read_u32();
                        let mut vec = Vec::new();
                        for i in 0..count {
                            let tmp =  reader.read_u32();
                            vec.push(tmp)
                        }
                    }
                    if flags & OrderFields::ExtraLocation as i16 > 0 {
                        let pos =  reader.read_i32();                        
                    }
                    if flags & OrderFields::ExtraData as i16 > 0 {
                        let extradata =  reader.read_u32();
                    }
                    if flags & OrderFields::Grouped as i16 > 0 {
                        let count =  reader.read_i32();
                        let mut vec = Vec::new();
                        for i in 0..count {
                            let tmp =  reader.read_u32();
                            vec.push(tmp)
                        }
                    }
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
    println!("Done.");


    let font_data: &[u8] = include_bytes!("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf");
    let font: Font<'static> = Font::try_from_bytes(font_data).expect("could not load font");

    for (i, (client_id, player)) in game_information.players.iter().enumerate() {
        imageproc::drawing::draw_text_mut(&mut image, player.color, 10, 10 + i as u32 * 50, Scale {x: 40.0, y: 40.0},  &font, &player.name);
    }
    imageproc::drawing::draw_text_mut(&mut image, RED, 500, 10, Scale {x: 40.0, y: 40.0},  &font, "Attack/AssaultMove");
    imageproc::drawing::draw_text_mut(&mut image, GREEN, 500, 60, Scale {x: 40.0, y: 40.0},  &font, "Move");
    imageproc::drawing::draw_text_mut(&mut image, BLUE, 500, 110, Scale {x: 40.0, y: 40.0},  &font, "PlaceBuilding");

    // image.as_mut_rgba8().unwrap().draw_text(Rgba([255, 0 , 0, 255]), 10, 10, Scale {x: 40.0, y: 40.0}, "hello world");
    println!("Saving image.");

    image.save("output.png").expect("Could not save output image");
    println!("Finished");

    Ok(())
}

fn get_rhs(line: &str) -> &str {
   line.rsplitn(2, ' ').next().unwrap()
}
