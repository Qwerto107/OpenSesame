use super::network_error;
use crate::i18n::Message;
use socket2::{Domain, Socket, Type};
use std::{
    ffi::c_void,
    fs,
    io::{self, Write},
    net::{IpAddr, SocketAddr},
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HANDLE, HMODULE, INVALID_HANDLE_VALUE},
    System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
        LoadLibraryExW,
    },
    UI::Shell::IsUserAnAdmin,
};

const DLL: &[u8] = include_bytes!("../../third_party/windivert/WinDivert.dll");
const DRIVER: &[u8] = include_bytes!("../../third_party/windivert/WinDivert64.sys");
const LICENSE: &[u8] = include_bytes!("../../third_party/windivert/LICENSE.txt");
const SOURCE: &[u8] = include_bytes!("../../third_party/windivert/WinDivert-2.2.2-Source.zip");

#[repr(C)]
struct Address {
    timestamp: i64,
    flags: u32,
    reserved: u32,
    data: [u8; 64],
}

impl Default for Address {
    fn default() -> Self {
        Self {
            timestamp: 0,
            flags: 0,
            reserved: 0,
            data: [0; 64],
        }
    }
}

type Open = unsafe extern "system" fn(*const u8, i32, i16, u64) -> HANDLE;
type Send = unsafe extern "system" fn(HANDLE, *const c_void, u32, *mut u32, *const Address) -> i32;
type Close = unsafe extern "system" fn(HANDLE) -> i32;
type Checksums = unsafe extern "system" fn(*mut c_void, u32, *mut Address, u64) -> i32;

struct Library {
    module: HMODULE,
    open: Open,
    send: Send,
    close: Close,
    checksums: Checksums,
}

impl Library {
    fn load(directory: &Path) -> Result<Self, Message> {
        let path = fs::canonicalize(directory.join("WinDivert.dll")).map_err(network_error)?;
        let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // 只从指定绝对目录加载 DLL，其依赖仅允许来自该目录或 System32。
        unsafe {
            let module = LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            );
            if module.is_null() {
                return Err(network_error(io::Error::last_os_error()));
            }
            let open = GetProcAddress(module, c"WinDivertOpen".as_ptr().cast());
            let send = GetProcAddress(module, c"WinDivertSend".as_ptr().cast());
            let close = GetProcAddress(module, c"WinDivertClose".as_ptr().cast());
            let checksums = GetProcAddress(module, c"WinDivertHelperCalcChecksums".as_ptr().cast());
            match (open, send, close, checksums) {
                (Some(open), Some(send), Some(close), Some(checksums)) => Ok(Self {
                    module,
                    open: std::mem::transmute::<unsafe extern "system" fn() -> isize, Open>(open),
                    send: std::mem::transmute::<unsafe extern "system" fn() -> isize, Send>(send),
                    close: std::mem::transmute::<unsafe extern "system" fn() -> isize, Close>(
                        close,
                    ),
                    checksums: std::mem::transmute::<unsafe extern "system" fn() -> isize, Checksums>(
                        checksums,
                    ),
                }),
                _ => {
                    FreeLibrary(module);
                    Err("The WinDivert library is incompatible.".into())
                }
            }
        }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // 所有发包句柄先关闭，最后释放其函数指针所属模块。
        unsafe {
            FreeLibrary(self.module);
        }
    }
}

pub(super) struct Sender {
    library: Library,
    handle: HANDLE,
    source: Socket,
    destination: IpAddr,
}

// Sender 只由单个异步任务持有；WinDivert 句柄和 DLL 模块允许跨线程使用。
unsafe impl std::marker::Send for Sender {}

impl Sender {
    pub(super) fn new(destination: SocketAddr) -> Result<Self, Message> {
        // 未提升权限时直接提示，避免释放文件或触发驱动安装尝试。
        if unsafe { IsUserAnAdmin() } == 0 {
            return Err(
                "Run OpenSesame as Administrator to send custom TCP flags on Windows.".into(),
            );
        }
        let directory = runtime_directory()?;
        let library = Library::load(&directory)?;
        let route =
            std::net::UdpSocket::bind(super::bind_address(destination)).map_err(network_error)?;
        route.connect(destination).map_err(network_error)?;
        let local = route.local_addr().map_err(network_error)?;
        let source = Socket::new(
            Domain::for_address(local),
            Type::STREAM,
            Some(socket2::Protocol::TCP),
        )
        .map_err(network_error)?;
        source
            .bind(&SocketAddr::new(local.ip(), 0).into())
            .map_err(network_error)?;
        // false + SEND_ONLY 只注入本程序构造的报文，不捕获或拦截其他流量。
        let handle = unsafe { (library.open)(c"false".as_ptr().cast(), 0, 0, 0x0008) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(network_error(io::Error::last_os_error()));
        }
        Ok(Self {
            library,
            handle,
            source,
            destination: destination.ip(),
        })
    }

    pub(super) fn send(&self, port: u16, flags: u8) -> Result<(), Message> {
        let source = self
            .source
            .local_addr()
            .map_err(network_error)?
            .as_socket()
            .ok_or("Could not determine the TCP source address.")?;
        let mut packet = packet(source, SocketAddr::new(self.destination, port), flags);
        let mut address = Address {
            flags: (1 << 17) | (u32::from(self.destination.is_ipv6()) << 20),
            ..Address::default()
        };
        let mut sent = 0;
        // Address 使用上游 WINDIVERT_ADDRESS 的 80 字节 ABI，缓冲区在调用期间有效。
        unsafe {
            if (self.library.checksums)(
                packet.as_mut_ptr().cast(),
                packet.len() as u32,
                &mut address,
                0,
            ) == 0
            {
                return Err("Could not calculate the TCP packet checksum.".into());
            }
            if (self.library.send)(
                self.handle,
                packet.as_ptr().cast(),
                packet.len() as u32,
                &mut sent,
                &address,
            ) == 0
            {
                return Err(network_error(io::Error::last_os_error()));
            }
        }
        if sent as usize != packet.len() {
            return Err("The TCP packet was not fully sent.".into());
        }
        Ok(())
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        unsafe {
            (self.library.close)(self.handle);
        }
    }
}

fn packet(source: SocketAddr, destination: SocketAddr, flags: u8) -> Vec<u8> {
    let mut packet = match (source.ip(), destination.ip()) {
        (IpAddr::V4(src), IpAddr::V4(dst)) => {
            let mut bytes = vec![0; 40];
            bytes[0] = 0x45;
            bytes[2..4].copy_from_slice(&40u16.to_be_bytes());
            bytes[8] = 64;
            bytes[9] = 6;
            bytes[12..16].copy_from_slice(&src.octets());
            bytes[16..20].copy_from_slice(&dst.octets());
            bytes
        }
        (IpAddr::V6(src), IpAddr::V6(dst)) => {
            let mut bytes = vec![0; 60];
            bytes[0] = 0x60;
            bytes[4..6].copy_from_slice(&20u16.to_be_bytes());
            bytes[6] = 6;
            bytes[7] = 64;
            bytes[8..24].copy_from_slice(&src.octets());
            bytes[24..40].copy_from_slice(&dst.octets());
            bytes
        }
        _ => unreachable!("source and destination address families must match"),
    };
    let offset = packet.len() - 20;
    let tcp = &mut packet[offset..];
    tcp[..2].copy_from_slice(&source.port().to_be_bytes());
    tcp[2..4].copy_from_slice(&destination.port().to_be_bytes());
    tcp[4..8].copy_from_slice(&1u32.to_be_bytes());
    if flags & 0x10 != 0 {
        tcp[8..12].copy_from_slice(&1u32.to_be_bytes());
    }
    tcp[12] = 0x50;
    tcp[13] = flags;
    tcp[14..16].copy_from_slice(&1024u16.to_be_bytes());
    if flags & 0x20 != 0 {
        tcp[18..20].copy_from_slice(&1u16.to_be_bytes());
    }
    packet
}

fn runtime_directory() -> Result<PathBuf, Message> {
    // 显式选择的替代库用于兼容替换版本；默认始终校验内嵌文件，避免加载被改写的缓存。
    let args: Vec<_> = std::env::args_os().collect();
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--windivert-dir") {
        return fs::canonicalize(&pair[1]).map_err(network_error);
    }
    let base = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set.")?;
    let directory = PathBuf::from(base)
        .join("OpenSesame")
        .join("WinDivert")
        .join("2.2.2-A");
    fs::create_dir_all(&directory).map_err(network_error)?;
    if fs::symlink_metadata(&directory)
        .map_err(network_error)?
        .file_attributes()
        & 0x400
        != 0
    {
        return Err("The WinDivert cache is invalid. Remove it and try again.".into());
    }
    for (name, bytes) in [
        ("WinDivert.dll", DLL),
        ("WinDivert64.sys", DRIVER),
        ("LICENSE.txt", LICENSE),
    ] {
        write_verified(&directory.join(name), bytes).map_err(network_error)?;
    }
    Ok(directory)
}

fn write_verified(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if !path.exists() {
        let temporary = path.with_extension(format!("{}.tmp", crate::model::new_id()));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            // 硬链接原子发布完整文件；并发释放不能覆盖已经存在的版本。
            match fs::hard_link(&temporary, path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
                Err(error) => Err(error),
            }
        })();
        let _ = fs::remove_file(temporary);
        result?;
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_attributes() & 0x400 != 0
        || !metadata.is_file()
        || metadata.len() != bytes.len() as u64
        || fs::read(path)? != bytes
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The WinDivert cache is invalid. Remove it and try again.",
        ));
    }
    Ok(())
}

pub fn export_source(directory: &Path) -> Result<(), Message> {
    fs::create_dir_all(directory).map_err(network_error)?;
    write_verified(&directory.join("WinDivert-2.2.2-Source.zip"), SOURCE).map_err(network_error)?;
    write_verified(&directory.join("LICENSE.txt"), LICENSE).map_err(network_error)
}
