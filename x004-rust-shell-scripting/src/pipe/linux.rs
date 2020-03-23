use crate::deps::{
    failure::{bail, Error},
    futures::{AsyncWrite, AsyncRead, AsyncWriteExt, AsyncReadExt},
    libc,
    struthionidae::sync::Mutex,
    struthionidae::logging::{info, debug, error},
};
use crate::deps::os_pipe as imp;
use futures::task::{Waker, Context, Poll, SpawnExt, LocalSpawnExt};
use std::io::{self, prelude::*};
use std::os::unix::prelude::*;
use std::collections::{HashMap, VecDeque};
use std::ffi::CStr;
use std::fs;
use std::fmt;
use std::pin::Pin;
use std::net::Shutdown;
use std::convert::TryInto;

use crate::deps::lazy_static::lazy_static;
use futures::{Future, Stream, FutureExt, Sink, StreamExt};
use std::process::exit;
use failure::_core::fmt::Formatter;
use bytes::{Bytes, BytesMut};

lazy_static! {
        static ref EPOLL_DRIVER: Epoll = Epoll::new();
}

pub fn async_pipe_driver() -> EpollFuture {
    EpollFuture(&*EPOLL_DRIVER)
}


/// Wraps the epoll libc interface.
struct Epoll {
    epfd: libc::c_int,
    wakers: Mutex<HashMap<RawFd, Vec<Waker>>>,
}

impl Epoll {
    //Constructor
    pub fn new() -> Epoll {
        let mut ret = Epoll {
            epfd: 0,
            wakers: Mutex::new(HashMap::new()),
        };
        unsafe {
            let r = libc::epoll_create1(0);
            if r == -1 {
                panic!("Epoll create returned -1");
                //There was an error
                //let e = libc::__errno_location()
                //In any case return the error
                //io::Error::new(ErrorKind::Other, "oh no!");
            }
            ret.epfd = r;
        }
        debug!("epoll:create epfd:{}", ret.epfd);
        ret
    }

    pub fn ctl_add_rawfd(&self, fd: RawFd, waker: Waker, events: u32) -> Result<(), io::Error> {
        let mut wakers = self.wakers.lock();
        let mut event = libc::epoll_event {
            events,
            u64: fd.try_into().unwrap(),
        };
        let r = unsafe { libc::epoll_ctl(self.epfd, libc::EPOLL_CTL_ADD, fd, &mut event) };
        if r == -1 {
            let strerror = unsafe {
                let se = libc::strerror(*libc::__errno_location());
                CStr::from_ptr(se).to_string_lossy()
            };
            debug!("epoll:ctl_add_rawfd:error: {}", strerror);
            return Err(io::Error::new(io::ErrorKind::Other, strerror));
        }
        if let Some(wakers) = wakers.insert(fd, vec![waker]) {
            for w in wakers {
                w.wake();
            }
        }
        debug!("epoll:ctl_add_rawfd fd:{} to epfd:{}", fd, self.epfd);
        Ok(())
    }

    pub fn ctl_mod_rawfd(&self, fd: RawFd, waker: Waker, events: u32) -> Result<(), io::Error> {
        let mut wakers = self.wakers.lock();
        let mut event = libc::epoll_event {
            events,
            u64: fd.try_into().unwrap(),
        };
        let r = unsafe { libc::epoll_ctl(self.epfd, libc::EPOLL_CTL_MOD, fd, &mut event) };
        if r == -1 {
            let strerror = unsafe {
                let se = libc::strerror(*libc::__errno_location());
                CStr::from_ptr(se).to_string_lossy()
            };
            debug!("epoll:ctl_mod_rawfd:error: {}", strerror);
            return Err(io::Error::new(io::ErrorKind::Other, strerror));
        }
        if let Some(wakers) = wakers.insert(fd, vec![waker]) {
            for w in wakers {
                w.wake();
            }
        }
        debug!("epoll:ctl_mod_rawfd fd:{} in epfd:{}", fd, self.epfd);
        Ok(())
    }

    pub fn ctl_del_rawfd(&self, fd: RawFd) -> Result<(), io::Error> {
        let mut wakers = self.wakers.lock();
        let mut event = libc::epoll_event {
            events: 0,
            u64: fd.try_into().unwrap(),
        };
        let r = unsafe { libc::epoll_ctl(self.epfd, libc::EPOLL_CTL_DEL, fd, &mut event) };
        if r == -1 {
            let strerror = unsafe {
                let se = libc::strerror(*libc::__errno_location());
                CStr::from_ptr(se).to_string_lossy()
            };
            debug!("epoll:ctl_del_rawfd:error: {}", strerror);
            return Err(io::Error::new(io::ErrorKind::Other, strerror));
        }
        if let Some(wakers) = wakers.remove(&fd) {
            for w in wakers {
                w.wake();
            }
        }
        debug!("epoll:ctl_del_rawfd: fd:{} from epfd:{}", fd, self.epfd);
        Ok(())
    }

    pub fn wait(&self, timeout: i32) -> Result<(), io::Error> {
        let mut events: Vec<libc::epoll_event> = vec![libc::epoll_event { events: 0, u64: 0 }; 10];
        debug!("enter epoll wait");
        let nfds =
            unsafe { libc::epoll_wait(self.epfd, &mut events[0], events.len() as i32, timeout) };
        if nfds == -1 {
            let strerror = unsafe {
                let se = libc::strerror(*libc::__errno_location());
                CStr::from_ptr(se).to_string_lossy()
            };
            error!("epoll:wait: error: {}", strerror);

            return Err(io::Error::new(io::ErrorKind::Other, strerror));
        }
        //Turn the events array into a vec of file handles
        for event in events.iter().take(nfds as usize) {
            debug!("epoll:wait: event:{} for fd:{}", event.events, event.u64);
            {
                let mut wakers = self.wakers.lock();
                if let Some(wakers) = wakers.remove(&(event.u64 as RawFd)) {
                    for w in wakers {
                        w.wake();
                    }
                }
            }
        }
        debug!("exit epoll wait");
        Ok(())
    }
}

pub struct EpollFuture(&'static Epoll);

impl Future for EpollFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.wait(100);
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}


impl AsRawFd for Epoll {
    fn as_raw_fd(self: &Self) -> i32 {
        self.epfd
    }
}

impl Drop for Epoll {
    fn drop(&mut self) {
        //Drop this thing
        let r = unsafe { libc::close(self.epfd) };
        if r == -1 {
            let strerror = unsafe {
                let se = libc::strerror(*libc::__errno_location());
                CStr::from_ptr(se).to_string_lossy()
            };
            debug!("epoll:drop:error: {}", strerror);
        }
    }
}

fn fncntl_set_nonblock(fd: RawFd) -> Result<(), Error> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
    if flags == -1 {
        let strerror = unsafe {
            let se = libc::strerror(*libc::__errno_location());
            CStr::from_ptr(se).to_string_lossy()
        };
        bail!("could not get file descriptor flags for fd={}; error: {}", fd, strerror);
    }


    let returncode = unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) };
    if returncode == -1 {
        let strerror = unsafe {
            let se = libc::strerror(*libc::__errno_location());
            CStr::from_ptr(se).to_string_lossy()
        };
        bail!("could not set O_NONBLOCK on fd={}; error: {}", fd, strerror);
    }

    Ok(())
}


pub struct PipeWriter {
    inner: imp::PipeWriter,
    eph: bool,
    epoll: &'static Epoll,
}

impl fmt::Debug for PipeWriter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PipeWriter(fd: {})", self.inner.as_raw_fd())
    }
}
//
//impl Drop for PipeWriter {
//    fn drop(&mut self) {
//        if self.eph {
//            self.epoll.ctl_del_rawfd(self.inner.as_raw_fd());
//        }
//    }
//}

impl PipeWriter {
    pub fn new(inner: imp::PipeWriter) -> Self {
        fncntl_set_nonblock(inner.as_raw_fd()).unwrap();
        Self {
            inner,
            eph: false,
            epoll: &(*EPOLL_DRIVER),
        }
    }
    fn on_would_block(&mut self, cx: &mut Context) -> Result<(), io::Error> {
        const EVENTS: u32 = (libc::EPOLLET | libc::EPOLLOUT | libc::EPOLLONESHOT) as u32;
        debug!("writer would block");
        if self.eph {
            self.epoll
                .ctl_mod_rawfd(self.inner.as_raw_fd(), cx.waker().clone(), EVENTS as u32)?;
        } else {
            self.epoll
                .ctl_add_rawfd(self.inner.as_raw_fd(), cx.waker().clone(), EVENTS as u32)?;
            self.eph = true;
        }
        Ok(())
    }
}

impl From<imp::PipeWriter> for PipeWriter {
    fn from(inner: imp::PipeWriter) -> Self {
        PipeWriter::new(inner)
    }
}

    impl IntoRawFd for PipeWriter {
        fn into_raw_fd(self) -> RawFd {
            self.inner.into_raw_fd()
        }
    }

impl AsRawFd for PipeWriter {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl FromRawFd for PipeWriter {
    unsafe fn from_raw_fd(fd: RawFd) -> PipeWriter {
        PipeWriter::new(imp::PipeWriter::from_raw_fd(fd))
    }
}

impl AsyncWrite for PipeWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.inner.write(buf) {
            Ok(i) => Poll::Ready(Ok(i)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Err(err) = self.on_would_block(cx) {
                    return Poll::Ready(Err(err));
                }
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        match self.inner.flush() {
            Ok(i) => Poll::Ready(Ok(())),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Err(err) = self.on_would_block(cx) {
                    return Poll::Ready(Err(err));
                }
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        match self.inner.flush() {
            Ok(i) => Poll::Ready(Ok(())),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Err(err) = self.on_would_block(cx) {
                    return Poll::Ready(Err(err));
                }
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

pub struct PipeReader {
    inner: imp::PipeReader,
    eph: bool,
    epoll: &'static Epoll,
}

//impl Drop for PipeReader {
//    fn drop(&mut self) {
//        if self.eph {
//            self.epoll.ctl_del_rawfd(self.inner.as_raw_fd());
//        }
//    }
//}

impl PipeReader {
    pub fn new(inner: imp::PipeReader) -> Self {
        fncntl_set_nonblock(inner.as_raw_fd()).unwrap();

        Self {
            inner,
            eph: false,
            epoll: &(*EPOLL_DRIVER),
        }
    }
    fn on_would_block(&mut self, cx: &mut Context) -> Result<(), io::Error> {
        const EVENTS: u32 = (libc::EPOLLET | libc::EPOLLIN | libc::EPOLLONESHOT) as u32;
        debug!("reader would block");

        if self.eph {
            self.epoll
                .ctl_mod_rawfd(self.inner.as_raw_fd(), cx.waker().clone(), EVENTS)?;
        } else {
            self.epoll
                .ctl_add_rawfd(self.inner.as_raw_fd(), cx.waker().clone(), EVENTS)?;
            self.eph = true;
        }
        Ok(())
    }
}

impl fmt::Debug for PipeReader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PipeReader(fd: {})", self.inner.as_raw_fd())
    }
}

impl From<imp::PipeReader> for PipeReader {
    fn from(inner: imp::PipeReader) -> Self {
        PipeReader::new(inner)
    }
}

    impl IntoRawFd for PipeReader {
        fn into_raw_fd(self) -> RawFd {
            self.inner.into_raw_fd()
        }
    }

impl AsRawFd for PipeReader {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl FromRawFd for PipeReader {
    unsafe fn from_raw_fd(fd: RawFd) -> PipeReader {
        PipeReader::new(imp::PipeReader::from_raw_fd(fd))
    }
}

impl AsyncRead for PipeReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.inner.read(buf) {
            Ok(i) => {
                if i == 0 {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(i))
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Err(err) = self.on_would_block(cx) {
                    return Poll::Ready(Err(err));
                }
                //cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}


pub struct Pipe {
    _no_direct_init: std::marker::PhantomData<()>
}

impl Pipe {
    pub fn new() -> Result<(PipeReader, PipeWriter), Error> {
        let (r, w) = imp::pipe()?;
        Ok((PipeReader::new(r), PipeWriter::new(w)))
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PipeStatus {
    Ok,
    Closed,
}

pub struct PipeStream {
    inner: PipeReader,
    status: PipeStatus,
}

impl PipeStream {
    pub fn new(inner: PipeReader) -> Self {
        Self {
            inner,
            status: PipeStatus::Ok,
        }
    }
}

impl Stream for PipeStream {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if self.as_ref().status == PipeStatus::Closed {
            return Poll::Ready(None);
        }

        let mut bytes = BytesMut::with_capacity(4096);
        unsafe { bytes.set_len(4096) };

        let stream = self.get_mut();
        let mut pin = Pin::new(&mut stream.inner);

        match pin.inner.read(bytes.as_mut()) {
            Ok(i) => {
                if i == 0 {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    unsafe { bytes.set_len(i) };
                    Poll::Ready(Some(Ok(bytes.freeze())))
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Err(err) = pin.on_would_block(cx) {
                    stream.status = PipeStatus::Closed;
                    return Poll::Ready(Some(Err(err)));
                }
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(e) => {
                stream.status = PipeStatus::Closed;
                Poll::Ready(Some(Err(e)))
            }
        }
    }
}


pub struct PipeSink {
    inner: PipeWriter,
    max_queue_bytes: usize,
    buffer: VecDeque<Bytes>,
    status: PipeStatus,
}

impl PipeSink {
    pub fn new(inner: PipeWriter) -> Self {
        Self {
            inner,
            max_queue_bytes: 1024 * 1024,
            buffer: VecDeque::new(),
            status: PipeStatus::Ok,
        }
    }
}

impl Sink<Bytes> for PipeSink {
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        if self.as_ref().status == PipeStatus::Closed {
            return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)));
        }
        let queue_bytes: usize = self.as_ref().buffer.iter().map(move |b| b.len()).sum();
        let sink = self.get_mut();
        if queue_bytes > sink.max_queue_bytes {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        if self.as_ref().status == PipeStatus::Closed {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        let sink = self.get_mut();
        sink.buffer.push_back(item);
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        if self.as_ref().status == PipeStatus::Closed {
            return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)));
        }
        let sink = self.get_mut();
        let bytes = match sink.buffer.front() {
            Some(bytes) => bytes,
            None => {
                return Poll::Ready(Ok(()));
            }
        };

        let mut pinned = Pin::new(&mut sink.inner);

        match pinned.inner.write(bytes.as_ref()) {
            Ok(i) => {
                sink.buffer.pop_front();
                // best effort "courtesy" flush
                match pinned.inner.flush() {
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        /* noop */
                    }
                    Err(e) => {
                        sink.status = PipeStatus::Closed;
                        return Poll::Ready(Err(e));
                    }
                };

                if sink.buffer.is_empty() {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Err(err) = pinned.on_would_block(cx) {
                    sink.status = PipeStatus::Closed;
                    return Poll::Ready(Err(err));
                }
                Poll::Pending
            }
            Err(e) => {
                sink.status = PipeStatus::Closed;
                Poll::Ready(Err(e))
            }
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        if self.as_ref().status == PipeStatus::Closed {
            return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)));
        }
        let sink = self.get_mut();
        let bytes = match sink.buffer.front() {
            Some(bytes) => bytes,
            None => {
                return Poll::Ready(Ok(()));
            }
        };

        let mut pinned = Pin::new(&mut sink.inner);

        match pinned.inner.write(bytes.as_ref()) {
            Ok(i) => {
                sink.buffer.pop_front();
                // best effort "courtesy" flush
                match pinned.inner.flush() {
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        /* noop */
                    }
                    Err(e) => {
                        sink.status = PipeStatus::Closed;
                        return Poll::Ready(Err(e));
                    }
                };

                if sink.buffer.is_empty() {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Err(err) = pinned.on_would_block(cx) {
                    sink.status = PipeStatus::Closed;
                    return Poll::Ready(Err(err));
                }
                Poll::Pending
            }
            Err(e) => {
                sink.status = PipeStatus::Closed;
                Poll::Ready(Err(e))
            }
        }
    }
}

#[test]
fn test_async_pipe_read_write() {
    use crate::deps::futures::executor::{self, ThreadPool};
    use crate::deps::struthionidae::sync::atomic::{Ordering, AtomicUsize};
    use crate::deps::struthionidae::logging::{info, debug, error, warn};

    crate::deps::struthionidae::logging::initialize();

    const XCOUNT: usize = 40;

    async fn write_to_pipe(mut w: PipeWriter) -> () {
        let s = "xxxxxxxx";
        for _ in 0..=((XCOUNT / s.len()) + 1) {
            let size = match w.write(s.as_bytes()).await {
                Ok(s) => s,
                Err(err) => {
                    match err.kind() {
                        io::ErrorKind::BrokenPipe => {
                            warn!("Broken Pipe {:?}", w);
                        }
                        _ => error!("{:?}", err),
                    }
                    return;
                }
            };
            //info!("wrote {} bytes", size);
        }
        ()
    }

    async fn read_from_pipe(mut r: PipeReader) {
        let mut buf = [0u8; 8];
        let counter = AtomicUsize::new(0);

        while counter.load(Ordering::Relaxed) < XCOUNT {
            let size = match r.read(&mut buf).await {
                Ok(s) => s,
                Err(err) => {
                    match err.kind() {
                        io::ErrorKind::BrokenPipe => {
                            warn!("Broken Pipe {:?}", r);
                        }
                        _ => error!("{:?}", err),
                    }
                    return;
                }
            };
            info!("read {} bytes; total: {} bytes", size, size + counter.load(Ordering::Relaxed));
            counter.fetch_add(size, Ordering::Relaxed);
        }
    }

    let (r, w) = Pipe::new().unwrap();
    let mut local_pool = ThreadPool::builder().name_prefix("ipc_async_test_thread").pool_size(2).create().unwrap();
    info!("spawning writer");
    local_pool.spawn(write_to_pipe(w)).unwrap();
    info!("spawning epoll driver");
    local_pool.spawn(async_pipe_driver());
    info!("spawning reader");
    executor::block_on(read_from_pipe(r));
}

#[test]
fn test_async_pipe_send_sink() {
    use crate::deps::futures::executor::{self, ThreadPool};
    use crate::deps::struthionidae::sync::atomic::{Ordering, AtomicUsize};
    use crate::deps::struthionidae::logging::{info, debug, error, warn};

    crate::deps::struthionidae::logging::initialize();

    const XCOUNT: usize = 4096 * 4;

    async fn write_to_pipe(mut w: PipeWriter) -> () {
        let s = vec![1u8; 4096];
        for _ in 0..=((XCOUNT / s.len()) + 1) {
            let size = match w.write(s.as_ref()).await {
                Ok(s) => {
                    s
                }
                Err(err) => {
                    match err.kind() {
                        io::ErrorKind::BrokenPipe => {
                            warn!("Broken Pipe {:?}", w);
                        }
                        _ => error!("{:?}", err),
                    }
                    return;
                }
            };
            info!("wrote {} bytes", size);
        }
        ()
    }



    async fn read_from_pipe(mut r: PipeReader) {
        let mut buf = [0u8; 8];
        let mut counter = 0usize;
        use futures::StreamExt;

        let mut stream = PipeStream::new(r);
        while counter < XCOUNT {
            match stream.next().await {
                Some(Ok(b)) => {
                    info!("read {} bytes", b.len());
                    counter += b.len();
                }
                _ => break
            }
        }

//            while counter.load(Ordering::Relaxed) < XCOUNT {
//                let size = match r.n(&mut buf).await {
//                    Ok(s) => s,
//                    Err(err) => {
//                        match err.kind() {
//                            io::ErrorKind::BrokenPipe => {
//                                warn!("Broken Pipe {:?}", r);
//                            }
//                            _ => error!("{:?}", err),
//                        }
//                        return;
//                    }
//                };
//                info!("read {} bytes; total: {} bytes", size, size + counter.load(Ordering::Relaxed));
//                counter.fetch_add(size, Ordering::Relaxed);
//            }
    }

    let (r, w) = Pipe::new().unwrap();
    let mut local_pool = ThreadPool::builder().name_prefix("ipc_async_test_thread").pool_size(2).create().unwrap();
    info!("spawning epoll driver");
    local_pool.spawn(async_pipe_driver());
    info!("spawning writer");
    local_pool.spawn(write_to_pipe(w)).unwrap();
    info!("spawning reader");
    executor::block_on(read_from_pipe(r));
}
