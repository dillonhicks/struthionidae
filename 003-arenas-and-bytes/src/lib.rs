use std::fmt;
use std::os::raw::c_void;

pub(crate) mod deps {
    pub(crate) use struthionidae;
    pub(crate) use bytes;
    pub(crate) use libc;
}

use crate::deps::struthionidae::logging::info;

use crate::deps::libc;
use std::ptr::NonNull;
use std::ffi::CStr;
use std::alloc::alloc_zeroed;
use std::mem::MaybeUninit;

pub type MallocFnC = (unsafe extern "C" fn(libc::size_t) -> *mut c_void);
pub type MallocFnWasi = (unsafe extern "C" fn(i32) -> i32);
pub type FieldId = i32;

pub type ErrorCode = i32;

pub mod error_code {
    use crate::ErrorCode;
    pub const OK: ErrorCode = 0;
    pub const OUT_OF_MEMORY: ErrorCode = libc::ENOMEM as ErrorCode;
    pub const PANIC: ErrorCode = ErrorCode::max_value();
}


pub const STRING_FIELD: FieldId = 1;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Span {
    pub ptr: *mut c_void,
    pub len: libc::size_t,
}

#[no_mangle]
pub unsafe extern "C" fn rust_read_dynamically_sized_field_c(field_id: FieldId, malloc: MallocFnC, span: *mut Span) -> ErrorCode {
    match field_id {
        STRING_FIELD => {
            let data = "hello purple turtle!";
            let size = data.as_bytes().len() + 1;
            let raw_ptr: *mut c_void = (malloc)(size);
            let ptr: NonNull<u8> = match NonNull::new(raw_ptr) {
                Some(ptr) => ptr.cast(),
                None => return error_code::OUT_OF_MEMORY
            };
            std::ptr::copy(data.as_bytes().as_ptr(), ptr.as_ptr(), data.as_bytes().len());
            // NULL TERMINATOR
            std::ptr::copy(b"\0".as_ptr(), ptr.as_ptr().add(data.as_bytes().len()), 1);

            (*span) = Span {
                ptr: ptr.as_ptr().cast(),
                len: size
            };

        },
        _ => {
            return error_code::PANIC;
        }
    }

    error_code::OK
}

unsafe extern "C" fn c_malloc(size: libc::size_t) -> *mut c_void {
    use std::alloc::{alloc, Layout};
    alloc(Layout::from_size_align_unchecked(size,  std::mem::size_of::<libc::size_t>())).cast()
}

#[test]
pub fn test_alloc_into() {
    crate::deps::struthionidae::logging::initialize();
    use std::mem::MaybeUninit;
    let mut span  = MaybeUninit::uninit();

    assert_eq!(unsafe { read_dynamically_sized_field_c(STRING_FIELD,  c_malloc  ,  span.as_mut_ptr()) }, error_code::OK);
    let span = unsafe { span.assume_init() };

    let slice: &[u8] = unsafe { std::slice::from_raw_parts(span.ptr.cast(), span.len) };

    let s =CStr::from_bytes_with_nul(slice).unwrap();

    assert_eq!(s, CStr::from_bytes_with_nul(&b"hello purple turtle!\0"[..]).unwrap());

}


#[test]
pub fn test_bytes_as_arena() {
    crate::deps::struthionidae::logging::initialize();

    use crate::deps::bytes::BufMut;


    let mut buf = crate::deps::bytes::BytesMut::with_capacity(4096);
    buf.put(&b"Lorem ipsum dolor sit amet consectetur adipiscing elit sociosqu mus condimentum, fames semper quam dignissim nisl in porttitor sed cum hendrerit vehicula, pellentesque gravida sem penatibus tortor vitae rhoncus aptent torquent. Litora metus sed mi etiam orci mattis scelerisque commodo tortor sociis placerat, gravida nullam taciti pellentesque nunc varius pulvinar ridiculus viverra rhoncus interdum, mus platea dapibus purus vulputate eros nisl integer dui eu. Euismod cubilia venenatis conubia est lacinia eleifend mi rhoncus, leo egestas nam iaculis sed in vitae commodo metus, maecenas condimentum aliquam pretium inceptos urna diam."[..]);
    let buf= buf.freeze();


}