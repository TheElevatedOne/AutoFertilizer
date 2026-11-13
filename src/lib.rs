//! Monster Hunter: World plugin for Stracker's Loader. Collects the Botanical Research harvest box
//! into the item box after every quest, and keeps the four fertilizers topped up with research points.
#![allow(non_snake_case)]

pub mod config;
pub mod console;
pub mod farm;
pub mod game;
pub mod log;

#[cfg(windows)]
mod plugin {
    use std::{
        ffi::c_void,
        panic::{self, AssertUnwindSafe},
        path::PathBuf,
        time::{Duration, Instant},
    };

    use windows::Win32::{
        Foundation::HMODULE,
        System::{
            LibraryLoader::{GetModuleFileNameW, GetModuleHandleW},
            Memory::{MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, VirtualQuery},
            Threading::{CRITICAL_SECTION, EnterCriticalSection, LeaveCriticalSection},
        },
    };

    use crate::{
        config::Config,
        console::{Console, Request},
        game::{Memory, SAVE_ROOT_RVA, Settings, Tracker},
        log,
    };

    const DLL_PROCESS_ATTACH: u32 = 1;
    /// PE timestamp of `MonsterHunterWorld.exe` 15.23.00 (421810), the version the offsets are for.
    const TIMESTAMP: u32 = 0x66D1_3CB6;

    /// # Safety
    /// Only the Windows loader calls this.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut c_void) -> i32 {
        if reason == DLL_PROCESS_ATTACH {
            let dll_path = module_path(module);
            // The loader lock is held here, so the work happens on a thread of its own.
            std::thread::spawn(move || run(dll_path));
        }
        1
    }

    fn run(dll_path: PathBuf) {
        let dir = dll_path.parent().map(PathBuf::from).unwrap_or_default();
        log::init(&dir.join("AutoFertilizer.log"));
        panic::set_hook(Box::new(|info| log!("error: {info}")));
        log!("AutoFertilizer {}", env!("CARGO_PKG_VERSION"));

        let config_path = dir.join("AutoFertilizer-config.json");
        let mut config = load_config(&config_path);

        // SAFETY: a null name returns the game's own module.
        let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
            log!("error: can't find the game module");
            return;
        };
        let base = module.0 as usize;
        let timestamp = pe_timestamp(base);
        if timestamp != TIMESTAMP {
            log!(
                "error: unsupported game version (exe timestamp {timestamp:#X}, this build needs {TIMESTAMP:#X}). Nothing will be changed."
            );
            return;
        }

        let mut console = Console::connect();
        if console.connected() {
            log!("console connected");
        }

        let start = Instant::now();
        let mut tracker = Tracker::default();
        let mut memory = GameMemory;
        loop {
            let mut command = None;
            match console.poll() {
                Some(Ok(Request::Game(c))) => command = Some(c),
                Some(Ok(Request::Reload)) => {
                    config = load_config(&config_path);
                    console.send("config reloaded");
                }
                Some(Err(e)) => console.send(&e),
                None => {}
            }

            // Twice the time between checks, and some more for a slow frame or a loading screen.
            let max_step = (config.check_rate / 1000 * 2 + 10).min(u32::MAX as u64) as u32;
            let settings = Settings { event: config.event, add_missing: config.add_missing, max_step };
            let mut lines = Vec::new();
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                tracker.tick(&mut memory, base + SAVE_ROOT_RVA, settings, command, &mut |l| lines.push(l));
            }));
            if result.is_err() {
                tracker = Tracker::default();
            }
            let secs = start.elapsed().as_secs();
            for line in lines {
                log!("[{:02}:{:02}:{:02}] {line}", secs / 3600, secs / 60 % 60, secs % 60);
                if command.is_some() {
                    console.send(&line);
                }
            }
            std::thread::sleep(Duration::from_millis(config.check_rate));
        }
    }

    fn load_config(path: &std::path::Path) -> Config {
        let (config, problems) = Config::load(path);
        for problem in problems {
            log!("{problem}");
        }
        log!(
            "checking every {} ms, event prices {}, missing fertilizers {}",
            config.check_rate,
            if config.event { "on" } else { "off" },
            if config.add_missing { "are bought" } else { "are left alone" }
        );
        config
    }

    /// The game's own memory. Every access checks that the pages are committed and allow it first,
    /// so a pointer the game has freed or not set up yet fails instead of crashing the game.
    struct GameMemory;

    fn accessible(addr: usize, len: usize, write: bool) -> bool {
        const READ: u32 = 0x02 | 0x04 | 0x08 | 0x20 | 0x40 | 0x80;
        const WRITE: u32 = 0x04 | 0x08 | 0x40 | 0x80;
        let end = addr.saturating_add(len);
        let mut at = addr;
        while at < end {
            let mut info = MEMORY_BASIC_INFORMATION::default();
            // SAFETY: VirtualQuery only fills in `info`.
            let got = unsafe {
                VirtualQuery(Some(at as *const c_void), &mut info, size_of::<MEMORY_BASIC_INFORMATION>())
            };
            let protect = info.Protect.0;
            if got == 0
                || info.State != MEM_COMMIT
                || protect & (PAGE_GUARD.0 | PAGE_NOACCESS.0) != 0
                || protect & if write { WRITE } else { READ } == 0
            {
                return false;
            }
            at = info.BaseAddress as usize + info.RegionSize;
        }
        addr != 0
    }

    impl Memory for GameMemory {
        fn read(&self, addr: usize, buf: &mut [u8]) -> bool {
            if !accessible(addr, buf.len(), false) {
                return false;
            }
            // SAFETY: the range was just checked to be committed and readable.
            unsafe { std::ptr::copy_nonoverlapping(addr as *const u8, buf.as_mut_ptr(), buf.len()) };
            true
        }

        fn write(&mut self, addr: usize, data: &[u8]) -> bool {
            if !accessible(addr, data.len(), true) {
                return false;
            }
            // SAFETY: the range was just checked to be committed and writable.
            unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), addr as *mut u8, data.len()) };
            true
        }

        fn enter(&mut self, addr: usize) -> bool {
            if !accessible(addr, size_of::<CRITICAL_SECTION>(), true) {
                return false;
            }
            // SAFETY: the slot's critical section, which the game sets up with the save data and
            // never frees while the save data is there.
            unsafe { EnterCriticalSection(addr as *mut CRITICAL_SECTION) };
            true
        }

        fn leave(&mut self, addr: usize) {
            // SAFETY: entered by `enter` on this thread.
            unsafe { LeaveCriticalSection(addr as *mut CRITICAL_SECTION) };
        }
    }

    fn pe_timestamp(base: usize) -> u32 {
        // SAFETY: the PE headers of a loaded module are mapped and readable.
        unsafe {
            let e_lfanew = *((base + 0x3C) as *const u32) as usize;
            *((base + e_lfanew + 8) as *const u32)
        }
    }

    fn module_path(module: HMODULE) -> PathBuf {
        let mut buffer = [0u16; 1024];
        // SAFETY: the buffer is valid and the module is this DLL, which is loaded.
        let len = unsafe { GetModuleFileNameW(Some(module), &mut buffer) } as usize;
        PathBuf::from(String::from_utf16_lossy(&buffer[..len]))
    }
}
