//! Host-side Lua bring-up seam for `ech-lua`.
//!
//! The upstream runtime stays under `third_party/curated/lua/`.
//! This module owns the restricted library surface and the `echos.*` host API.

use alloc::string::{String, ToString};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EchLuaError {
    RuntimeUnavailable(&'static str),
    InteriorNul,
    Init(String),
    Load(String),
    Runtime(String),
    Type(String),
}

pub type Result<T> = core::result::Result<T, EchLuaError>;

#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
mod host {
    use super::{EchLuaError, Result};
    use alloc::{
        format,
        string::{String, ToString},
        vec::Vec,
    };
    use core::ffi::{c_char, c_int, c_void, CStr};
    use core::ptr::{null, NonNull};
    use std::ffi::CString;
    use std::fs;

    const LUA_OK: c_int = 0;
    const LUA_TBOOLEAN: c_int = 1;
    const LUA_TNUMBER: c_int = 3;
    const LUA_TSTRING: c_int = 4;

    #[repr(C)]
    struct lua_State {
        _private: [u8; 0],
    }

    type LuaCFunction = Option<unsafe extern "C" fn(*mut lua_State) -> c_int>;

    unsafe extern "C" {
        fn lua_close(state: *mut lua_State);
        fn lua_settop(state: *mut lua_State, idx: c_int);
        fn lua_type(state: *mut lua_State, idx: c_int) -> c_int;
        fn lua_toboolean(state: *mut lua_State, idx: c_int) -> c_int;
        fn lua_tolstring(state: *mut lua_State, idx: c_int, len: *mut usize) -> *const c_char;
        fn lua_pushnil(state: *mut lua_State);
        fn lua_pushlstring(state: *mut lua_State, s: *const c_char, len: usize) -> *const c_char;
        fn lua_pushcclosure(state: *mut lua_State, function: LuaCFunction, upvalues: c_int);
        fn lua_pushboolean(state: *mut lua_State, value: c_int);
        fn lua_createtable(state: *mut lua_State, narr: c_int, nrec: c_int);
        fn lua_setglobal(state: *mut lua_State, name: *const c_char);
        fn lua_setfield(state: *mut lua_State, idx: c_int, key: *const c_char);
        fn lua_pcallk(
            state: *mut lua_State,
            nargs: c_int,
            nresults: c_int,
            errfunc: c_int,
            ctx: isize,
            k: *const c_void,
        ) -> c_int;
        fn luaL_checklstring(state: *mut lua_State, arg: c_int, len: *mut usize) -> *const c_char;
        fn luaL_loadbufferx(
            state: *mut lua_State,
            buffer: *const c_char,
            size: usize,
            name: *const c_char,
            mode: *const c_char,
        ) -> c_int;
        fn luaL_newstate() -> *mut lua_State;
        fn luaL_requiref(
            state: *mut lua_State,
            modname: *const c_char,
            openf: LuaCFunction,
            glb: c_int,
        );

        fn luaopen_base(state: *mut lua_State) -> c_int;
        fn luaopen_coroutine(state: *mut lua_State) -> c_int;
        fn luaopen_math(state: *mut lua_State) -> c_int;
        fn luaopen_string(state: *mut lua_State) -> c_int;
        fn luaopen_table(state: *mut lua_State) -> c_int;
        fn luaopen_utf8(state: *mut lua_State) -> c_int;
    }

    fn pop(state: *mut lua_State, count: c_int) {
        unsafe {
            lua_settop(state, -count - 1);
        }
    }

    fn push_string(state: *mut lua_State, value: &str) {
        unsafe {
            lua_pushlstring(state, value.as_ptr().cast::<c_char>(), value.len());
        }
    }

    fn read_checked_string(state: *mut lua_State, index: c_int) -> String {
        let mut len = 0usize;
        let ptr = unsafe { luaL_checklstring(state, index, &mut len) };
        if ptr.is_null() {
            return String::new();
        }
        String::from_utf8_lossy(unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len) })
            .into_owned()
    }

    fn stack_string(state: *mut lua_State, index: c_int) -> String {
        let mut len = 0usize;
        let ptr = unsafe { lua_tolstring(state, index, &mut len) };
        if ptr.is_null() {
            return String::new();
        }
        String::from_utf8_lossy(unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len) })
            .into_owned()
    }

    unsafe extern "C" fn lua_echos_exec(state: *mut lua_State) -> c_int {
        let command = read_checked_string(state, 1);
        let output = crate::shell::run_command(&command).unwrap_or_default();
        push_string(state, output.trim_end_matches('\n'));
        1
    }

    unsafe extern "C" fn lua_echos_exists(state: *mut lua_State) -> c_int {
        let path = read_checked_string(state, 1);
        unsafe { lua_pushboolean(state, if fs::metadata(&path).is_ok() { 1 } else { 0 }) };
        1
    }

    unsafe extern "C" fn lua_echos_readfile(state: *mut lua_State) -> c_int {
        let path = read_checked_string(state, 1);
        match fs::read_to_string(&path) {
            Ok(contents) => {
                push_string(state, &contents);
                1
            }
            Err(error) => {
                unsafe { lua_pushnil(state) };
                push_string(state, &error.to_string());
                2
            }
        }
    }

    unsafe extern "C" fn lua_echos_writefile(state: *mut lua_State) -> c_int {
        let path = read_checked_string(state, 1);
        let contents = read_checked_string(state, 2);
        match fs::write(&path, contents.as_bytes()) {
            Ok(()) => {
                unsafe { lua_pushboolean(state, 1) };
                1
            }
            Err(error) => {
                unsafe { lua_pushnil(state) };
                push_string(state, &error.to_string());
                2
            }
        }
    }

    fn c_name(bytes: &'static [u8]) -> *const c_char {
        bytes.as_ptr().cast::<c_char>()
    }

    fn require_lib(state: *mut lua_State, name: &'static [u8], openf: LuaCFunction) {
        unsafe {
            luaL_requiref(state, c_name(name), openf, 1);
        }
        pop(state, 1);
    }

    fn install_safe_libs(state: *mut lua_State) {
        require_lib(state, b"_G\0", Some(luaopen_base));
        require_lib(state, b"coroutine\0", Some(luaopen_coroutine));
        require_lib(state, b"math\0", Some(luaopen_math));
        require_lib(state, b"string\0", Some(luaopen_string));
        require_lib(state, b"table\0", Some(luaopen_table));
        require_lib(state, b"utf8\0", Some(luaopen_utf8));
    }

    fn install_echos_module(state: *mut lua_State) {
        unsafe {
            lua_createtable(state, 0, 4);
            lua_pushcclosure(state, Some(lua_echos_exec), 0);
            lua_setfield(state, -2, c_name(b"exec\0"));
            lua_pushcclosure(state, Some(lua_echos_exists), 0);
            lua_setfield(state, -2, c_name(b"exists\0"));
            lua_pushcclosure(state, Some(lua_echos_readfile), 0);
            lua_setfield(state, -2, c_name(b"readfile\0"));
            lua_pushcclosure(state, Some(lua_echos_writefile), 0);
            lua_setfield(state, -2, c_name(b"writefile\0"));
            lua_setglobal(state, c_name(b"echos\0"));
        }
    }

    pub struct EchLua {
        state: NonNull<lua_State>,
    }

    impl EchLua {
        pub fn new() -> Result<Self> {
            let state = NonNull::new(unsafe { luaL_newstate() })
                .ok_or_else(|| EchLuaError::Init(String::from("luaL_newstate returned null")))?;
            install_safe_libs(state.as_ptr());
            install_echos_module(state.as_ptr());
            Ok(Self { state })
        }

        pub fn run_chunk(&mut self, chunk: &str) -> Result<()> {
            self.load_chunk(chunk)?;
            let rc = unsafe { lua_pcallk(self.state.as_ptr(), 0, 0, 0, 0, null()) };
            if rc != LUA_OK {
                let message = stack_string(self.state.as_ptr(), -1);
                pop(self.state.as_ptr(), 1);
                return Err(EchLuaError::Runtime(message));
            }
            Ok(())
        }

        pub fn eval_to_string(&mut self, expr: &str) -> Result<String> {
            let chunk = format!("return ({expr})");
            self.load_chunk(&chunk)?;
            let rc = unsafe { lua_pcallk(self.state.as_ptr(), 0, 1, 0, 0, null()) };
            if rc != LUA_OK {
                let message = stack_string(self.state.as_ptr(), -1);
                pop(self.state.as_ptr(), 1);
                return Err(EchLuaError::Runtime(message));
            }
            let kind = unsafe { lua_type(self.state.as_ptr(), -1) };
            let value = match kind {
                LUA_TSTRING | LUA_TNUMBER => stack_string(self.state.as_ptr(), -1),
                LUA_TBOOLEAN => {
                    if unsafe { lua_toboolean(self.state.as_ptr(), -1) } != 0 {
                        String::from("true")
                    } else {
                        String::from("false")
                    }
                }
                _ => {
                    pop(self.state.as_ptr(), 1);
                    return Err(EchLuaError::Type(format!(
                        "unsupported Lua return type {kind}"
                    )));
                }
            };
            pop(self.state.as_ptr(), 1);
            Ok(value)
        }

        pub fn eval_to_bool(&mut self, expr: &str) -> Result<bool> {
            let chunk = format!("return ({expr})");
            self.load_chunk(&chunk)?;
            let rc = unsafe { lua_pcallk(self.state.as_ptr(), 0, 1, 0, 0, null()) };
            if rc != LUA_OK {
                let message = stack_string(self.state.as_ptr(), -1);
                pop(self.state.as_ptr(), 1);
                return Err(EchLuaError::Runtime(message));
            }
            let kind = unsafe { lua_type(self.state.as_ptr(), -1) };
            if kind != LUA_TBOOLEAN {
                pop(self.state.as_ptr(), 1);
                return Err(EchLuaError::Type(format!(
                    "expected boolean result, got Lua type {kind}"
                )));
            }
            let value = unsafe { lua_toboolean(self.state.as_ptr(), -1) } != 0;
            pop(self.state.as_ptr(), 1);
            Ok(value)
        }

        fn load_chunk(&mut self, chunk: &str) -> Result<()> {
            let name = CString::new("echos_chunk").map_err(|_| EchLuaError::InteriorNul)?;
            let rc = unsafe {
                luaL_loadbufferx(
                    self.state.as_ptr(),
                    chunk.as_ptr().cast::<c_char>(),
                    chunk.len(),
                    name.as_ptr(),
                    null(),
                )
            };
            if rc != LUA_OK {
                let message = stack_string(self.state.as_ptr(), -1);
                pop(self.state.as_ptr(), 1);
                return Err(EchLuaError::Load(message));
            }
            Ok(())
        }
    }

    impl Drop for EchLua {
        fn drop(&mut self) {
            unsafe {
                lua_close(self.state.as_ptr());
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::EchLua;
        use alloc::{format, string::String};
        use std::env;
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        fn unique_path(prefix: &str, suffix: &str) -> String {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            env::temp_dir()
                .join(format!("{prefix}-{stamp}.{suffix}"))
                .to_string_lossy()
                .into_owned()
        }

        fn lua_path(path: &str) -> String {
            path.replace('\\', "/")
        }

        #[test]
        fn lua_safe_libs_and_host_api_work() {
            let mut lua = EchLua::new().unwrap();
            assert_eq!(
                lua.eval_to_string("'hi-' .. string.upper('ok')").unwrap(),
                "hi-OK"
            );
            lua.run_chunk("assert(io == nil)\nassert(os == nil)")
                .unwrap();

            let path = lua_path(&unique_path("echos-lua", "txt"));
            lua.run_chunk(&format!(
                "assert(echos.writefile('{path}', 'payload'))\nassert(echos.exists('{path}'))"
            ))
            .unwrap();
            assert_eq!(
                lua.eval_to_string(&format!("echos.readfile('{path}')"))
                    .unwrap(),
                "payload"
            );
            assert_eq!(
                lua.eval_to_string("echos.exec('echo merhaba')").unwrap(),
                "merhaba"
            );
            let _ = fs::remove_file(path);
        }

        #[test]
        fn lua_boolean_eval_returns_native_bool() {
            let mut lua = EchLua::new().unwrap();
            assert!(lua.eval_to_bool("2 + 2 == 4").unwrap());
            assert!(!lua
                .eval_to_bool("echos.exists('definitely-missing')")
                .unwrap());
        }
    }
}

#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
pub use host::EchLua;

#[cfg(any(target_os = "none", target_os = "uefi"))]
pub struct EchLua;

#[cfg(any(target_os = "none", target_os = "uefi"))]
impl EchLua {
    pub fn new() -> Result<Self> {
        Err(EchLuaError::RuntimeUnavailable(
            "ech-lua host bring-up currently targets non-UEFI host builds only",
        ))
    }

    pub fn run_chunk(&mut self, _: &str) -> Result<()> {
        Err(EchLuaError::RuntimeUnavailable(
            "ech-lua host bring-up currently targets non-UEFI host builds only",
        ))
    }

    pub fn eval_to_string(&mut self, _: &str) -> Result<String> {
        Err(EchLuaError::RuntimeUnavailable(
            "ech-lua host bring-up currently targets non-UEFI host builds only",
        ))
    }

    pub fn eval_to_bool(&mut self, _: &str) -> Result<bool> {
        Err(EchLuaError::RuntimeUnavailable(
            "ech-lua host bring-up currently targets non-UEFI host builds only",
        ))
    }
}
