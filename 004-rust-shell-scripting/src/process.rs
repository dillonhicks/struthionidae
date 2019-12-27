use crate::deps::{
    failure::Error,
    futures::{executor::block_on, Future, task::Poll},
    struthionidae::logging::info,
};
use std::process as imp;
use std::{borrow::Cow, fmt, ffi::OsStr, fs, io::Read, path::{Path, PathBuf}};
use futures::task::SpawnExt;

pub struct Process {
    process: std::process::Child,
}

impl Process {
    const TAG: Cow<'static, str> = Cow::Borrowed("Command");

    pub fn new(cmd: &mut Command) -> Result<Self, Error> {
        let process = cmd.inner.spawn()?;
        Ok(Process {
            process,
        })
    }

    pub fn wait(&mut self) -> Result<std::process::ExitStatus, Error> {
        Ok(self.process.wait()?)
    }

    pub fn wait_with_output(mut self) -> Result<std::process::Output, Error> {
        Ok(self.process.wait_with_output()?)
    }
}

impl Future for Process {
    type Output = std::process::Output;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> Poll<Self::Output> {
        match self.process.try_wait() {
            Ok(Some(status)) => {
                info!("status READY");

                let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
                match ((&mut self.process).stdout.take(), (&mut self.process).stderr.take()) {
                    (None, None) => {}
                    (Some(mut out), None) => {
                        let res = out.read_to_end(&mut stdout);
                        res.unwrap();
                    }
                    (None, Some(mut err)) => {
                        let res = err.read_to_end(&mut stderr);
                        res.unwrap();
                    }
                    (Some(mut out), Some(mut err)) => {
                        let res = out
                            .read_to_end(&mut stdout)
                            .and_then(|size| err.read_to_end(&mut stderr).map(|s| size + s));
                        res.unwrap();
                    }
                }

                Poll::Ready(std::process::Output {
                    status,
                    stdout,
                    stderr,
                })
            }
            Ok(None) => {
                info!("status not ready yet, let's really wait");
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(e) => {
                info!("error attempting to wait: {}", e);
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}


#[derive(Debug)]
enum IoKind {
    Piped,
    Inherit,
    Null,
    File(fs::File),
    // Sink(Fd),
    // Source(Fd)
}

use std::os::unix::io::RawFd;

#[derive(Debug, Copy, Clone, PartialEq)]
enum Fd {
    DevNull,
    Stdin,
    Stdout,
    Stderr,
    Other(RawFd),
}


impl From<RawFd> for Fd {
    fn from(fd: RawFd) -> Self {
        match fd {
            std::i32::MIN..=-1 => Fd::DevNull,
            0 => Fd::Stdin,
            1 => Fd::Stdout,
            2 => Fd::Stderr,
            _ => Fd::Other(fd)
        }
    }
}


pub struct Stdio {
    io_kind: IoKind
}

impl fmt::Debug for Stdio {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Stdio({:?})", &self.io_kind)
    }
}

impl Into<imp::Stdio> for Stdio {
    fn into(self) -> imp::Stdio {
        match self.io_kind {
            IoKind::Null => imp::Stdio::null(),
            IoKind::Inherit => imp::Stdio::inherit(),
            IoKind::File(file) => imp::Stdio::from(file),
            IoKind::Piped => imp::Stdio::piped(),
//            IoKind::Sink(fd) => {imp::Stdio::from()}
//            IoKind::Source(fd) => {}
        }
    }
}

impl Stdio {
    pub fn null() -> Self {
        Stdio {
            io_kind: IoKind::Null
        }
    }
    pub fn inherit() -> Self {
        Stdio {
            io_kind: IoKind::Inherit
        }
    }
    pub fn piped() -> Self {
        Stdio {
            io_kind: IoKind::Piped
        }
    }

    pub fn redirect_to_out() -> Self {
        Stdio {
            io_kind: IoKind::Piped
        }
    }

    pub fn from_file(file: fs::File) -> Self {
        Stdio {
            io_kind: IoKind::File(file)
        }
    }
}


pub mod ipc {
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
            self.0.wait(1);
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

    impl Drop for PipeWriter {
        fn drop(&mut self) {
            if self.eph {
                self.epoll.ctl_del_rawfd(self.inner.as_raw_fd());
            }
        }
    }

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

//    impl IntoRawFd for PipeWriter {
//        fn into_raw_fd(self) -> RawFd {
//            self.inner.into_raw_fd()
//        }
//    }

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

    impl Drop for PipeReader {
        fn drop(&mut self) {
            if self.eph {
                self.epoll.ctl_del_rawfd(self.inner.as_raw_fd());
            }
        }
    }

    impl PipeReader {
        pub fn new(inner: imp::PipeReader) -> Self {
            fncntl_set_nonblock(inner.as_raw_fd()).unwrap();

            Self {
                inner,
                eph: false,
                epoll: &(*EPOLL_DRIVER),
            }
        }
        fn on_would_block(&mut self, cx: &mut Context) -> Result<(), io::Error>{
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

//    impl IntoRawFd for PipeReader {
//        fn into_raw_fd(self) -> RawFd {
//            self.inner.into_raw_fd()
//        }
//    }

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
                    }else {
                        Poll::Ready(Ok(i))
                    }
                },
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
                status: PipeStatus::Ok
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
                    }else {
                        unsafe { bytes.set_len(i) };
                        Poll::Ready(Some(Ok(bytes.freeze())))
                    }
                },
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
                },
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
                status: PipeStatus::Ok
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
            let  sink = self.get_mut();
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
                        Ok(_)  => {},
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            /* noop */
                        },
                        Err(e) => {
                            sink.status = PipeStatus::Closed;
                            return Poll::Ready(Err(e));
                        },
                    };

                    if sink.buffer.is_empty() {
                        Poll::Ready(Ok(()))
                    } else {
                        Poll::Pending
                    }
                },
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
                },
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
                        Ok(_) => {},
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            /* noop */
                        },
                        Err(e) => {
                            sink.status = PipeStatus::Closed;
                            return Poll::Ready(Err(e));
                        },
                    };

                    if sink.buffer.is_empty() {
                        Poll::Ready(Ok(()))
                    } else {
                        Poll::Pending
                    }
                },
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
                },
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
            for _ in 0..=((XCOUNT / s.len()) +1 ) {
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
        local_pool.spawn(EpollFuture(&*EPOLL_DRIVER));
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
            for _ in 0..=((XCOUNT / s.len()) +1 ) {
                let size = match w.write(s.as_ref()).await {
                    Ok(s) => {
                        s
                    },
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
            while counter < XCOUNT  {
                match  stream.next().await {
                    Some(Ok(b)) => {
                        info!("read {} bytes", b.len());
                        counter += b.len();
                    },
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
        local_pool.spawn(EpollFuture(&*EPOLL_DRIVER));
        info!("spawning writer");
        local_pool.spawn(write_to_pipe(w)).unwrap();
        info!("spawning reader");
        executor::block_on(read_from_pipe(r));
    }
}

/// A process builder, providing fine-grained control
/// over how a new process should be spawned.
///
/// A default configuration can be
/// generated using `Command::new(program)`, where `program` gives a path to the
/// program to be executed. Additional builder methods allow the configuration
/// to be changed (for example, by adding arguments) prior to spawning:
///
/// ```no_run
/// use std::process::Command;
///
/// let output = if cfg!(target_os = "windows") {
///     Command::new("cmd")
///             .args(&["/C", "echo hello"])
///             .output()
///             .expect("failed to execute process")
/// } else {
///     Command::new("sh")
///             .arg("-c")
///             .arg("echo hello")
///             .output()
///             .expect("failed to execute process")
/// };
///
/// let hello = output.stdout;
/// ```
///
/// `Command` can be reused to spawn multiple processes. The builder methods
/// change the command without needing to immediately spawn the process.
///
/// ```no_run
/// use std::process::Command;
///
/// let mut echo_hello = Command::new("sh");
/// echo_hello.arg("-c")
///           .arg("echo hello");
/// let hello_1 = echo_hello.output().expect("failed to execute process");
/// let hello_2 = echo_hello.output().expect("failed to execute process");
/// ```
///
/// Similarly, you can call builder methods after spawning a process and then
/// spawn a new process with the modified settings.
///
/// ```no_run
/// use std::process::Command;
///
/// let mut list_dir = Command::new("ls");
///
/// // Execute `ls` in the current directory of the program.
/// list_dir.status().expect("process failed to execute");
///
/// println!();
///
/// // Change `ls` to execute in the root directory.
/// list_dir.current_dir("/");
///
/// // And then execute `ls` again but in the root directory.
/// list_dir.status().expect("process failed to execute");
/// ```
#[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
pub struct Command {
    inner: imp::Command,

}


impl Command {
    /// Constructs a new `Command` for launching the program at
    /// path `program`, with the following default configuration:
    ///
    /// * No arguments to the program
    /// * Inherit the current process's environment
    /// * Inherit the current process's working directory
    /// * Inherit stdin/stdout/stderr for `spawn` or `status`, but create pipes for `output`
    ///
    /// Builder methods are provided to change these defaults and
    /// otherwise configure the process.
    ///
    /// If `program` is not an absolute path, the `PATH` will be searched in
    /// an OS-defined way.
    ///
    /// The search path to be used may be controlled by setting the
    /// `PATH` environment variable on the Command,
    /// but this has some implementation limitations on Windows
    /// (see issue #37519).
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("sh")
    ///         .spawn()
    ///         .expect("sh command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn new<S: AsRef<OsStr>>(program: S) -> Command {
        Command { inner: imp::Command::new(program.as_ref()) }
    }

    /// Adds an argument to pass to the program.
    ///
    /// Only one argument can be passed per use. So instead of:
    ///
    /// ```no_run
    /// # std::process::Command::new("sh")
    /// .arg("-C /path/to/repo")
    /// # ;
    /// ```
    ///
    /// usage would be:
    ///
    /// ```no_run
    /// # std::process::Command::new("sh")
    /// .arg("-C")
    /// .arg("/path/to/repo")
    /// # ;
    /// ```
    ///
    /// To pass multiple arguments see [`args`].
    ///
    /// [`args`]: #method.args
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .arg("-l")
    ///         .arg("-a")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Command {
        self.inner.arg(arg.as_ref());
        self
    }

    /// Adds multiple arguments to pass to the program.
    ///
    /// To pass a single argument see [`arg`].
    ///
    /// [`arg`]: #method.arg
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .args(&["-l", "-a"])
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn args<I, S>(&mut self, args: I) -> &mut Command
        where I: IntoIterator<Item=S>, S: AsRef<OsStr>
    {
        for arg in args {
            self.arg(arg.as_ref());
        }
        self
    }

    /// Inserts or updates an environment variable mapping.
    ///
    /// Note that environment variable names are case-insensitive (but case-preserving) on Windows,
    /// and case-sensitive on all other platforms.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .env("PATH", "/bin")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn env<K, V>(&mut self, key: K, val: V) -> &mut Command
        where K: AsRef<OsStr>, V: AsRef<OsStr>
    {
        self.inner.env(key.as_ref(), val.as_ref());
        self
    }

    /// Adds or updates multiple environment variable mappings.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::{Command, Stdio};
    /// use std::env;
    /// use std::collections::HashMap;
    ///
    /// let filtered_env : HashMap<String, String> =
    ///     env::vars().filter(|&(ref k, _)|
    ///         k == "TERM" || k == "TZ" || k == "LANG" || k == "PATH"
    ///     ).collect();
    ///
    /// Command::new("printenv")
    ///         .stdin(Stdio::null())
    ///         .stdout(Stdio::inherit())
    ///         .env_clear()
    ///         .envs(&filtered_env)
    ///         .spawn()
    ///         .expect("printenv failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "command_envs", since = "1.19.0"))]
    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Command
        where I: IntoIterator<Item=(K, V)>, K: AsRef<OsStr>, V: AsRef<OsStr>
    {
        for (ref key, ref val) in vars {
            self.inner.env(key.as_ref(), val.as_ref());
        }
        self
    }

    /// Removes an environment variable mapping.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .env_remove("PATH")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn env_remove<K: AsRef<OsStr>>(&mut self, key: K) -> &mut Command {
        self.inner.env_remove(key.as_ref());
        self
    }

    /// Clears the entire environment map for the child process.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .env_clear()
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn env_clear(&mut self) -> &mut Command {
        self.inner.env_clear();
        self
    }

    /// Sets the working directory for the child process.
    ///
    /// # Platform-specific behavior
    ///
    /// If the program path is relative (e.g., `"./script.sh"`), it's ambiguous
    /// whether it should be interpreted relative to the parent's working
    /// directory or relative to `current_dir`. The behavior in this case is
    /// platform specific and unstable, and it's recommended to use
    /// [`canonicalize`] to get an absolute program path instead.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .current_dir("/bin")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    ///
    /// [`canonicalize`]: ../fs/fn.canonicalize.html
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn current_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut Command {
        self.inner.current_dir(dir);
        self
    }

    /// Configuration for the child process's standard input (stdin) handle.
    ///
    /// Defaults to [`inherit`] when used with `spawn` or `status`, and
    /// defaults to [`piped`] when used with `output`.
    ///
    /// [`inherit`]: struct.Stdio.html#method.inherit
    /// [`piped`]: struct.Stdio.html#method.piped
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::{Command, Stdio};
    ///
    /// Command::new("ls")
    ///         .stdin(Stdio::null())
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn stdin<T: Into<imp::Stdio>>(&mut self, cfg: T) -> &mut Command {
        let cfg = cfg.into();
        self.inner.stdin(cfg);
        self
    }

    /// Configuration for the child process's standard output (stdout) handle.
    ///
    /// Defaults to [`inherit`] when used with `spawn` or `status`, and
    /// defaults to [`piped`] when used with `output`.
    ///
    /// [`inherit`]: struct.Stdio.html#method.inherit
    /// [`piped`]: struct.Stdio.html#method.piped
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::{Command, Stdio};
    ///
    /// Command::new("ls")
    ///         .stdout(Stdio::null())
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn stdout<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Command {
        let cfg = cfg.into();
        self.inner.stdout(cfg);
        self
    }

    /// Configuration for the child process's standard error (stderr) handle.
    ///
    /// Defaults to [`inherit`] when used with `spawn` or `status`, and
    /// defaults to [`piped`] when used with `output`.
    ///
    /// [`inherit`]: struct.Stdio.html#method.inherit
    /// [`piped`]: struct.Stdio.html#method.piped
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::{Command, Stdio};
    ///
    /// Command::new("ls")
    ///         .stderr(Stdio::null())
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn stderr<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Command {
        let cfg = cfg.into();
        self.inner.stderr(cfg);
        self
    }

    /// Executes the command as a child process, returning a handle to it.
    ///
    /// By default, stdin, stdout and stderr are inherited from the parent.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn spawn(&mut self) -> Result<Process, Error> {
        Process::new(self)
    }

    /// Executes the command as a child process, waiting for it to finish and
    /// collecting all of its output.
    ///
    /// By default, stdout and stderr are captured (and used to provide the
    /// resulting output). Stdin is not inherited from the parent and any
    /// attempt by the child process to read from the stdin stream will result
    /// in the stream immediately closing.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::process::Command;
    /// use std::io::{self, Write};
    /// let output = Command::new("/bin/cat")
    ///                      .arg("file.txt")
    ///                      .output()
    ///                      .expect("failed to execute process");
    ///
    /// println!("status: {}", output.status);
    /// io::stdout().write_all(&output.stdout).unwrap();
    /// io::stderr().write_all(&output.stderr).unwrap();
    ///
    /// assert!(output.status.success());
    /// ```
    #[cfg(feature = "DISABLED")]
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn output(&mut self) -> io::Result<Output> {
        self.inner.spawn(imp::Stdio::MakePipe, false).map(Child::from_inner)
            .and_then(|p| p.wait_with_output())
    }

    /// Executes a command as a child process, waiting for it to finish and
    /// collecting its exit status.
    ///
    /// By default, stdin, stdout and stderr are inherited from the parent.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// let status = Command::new("/bin/cat")
    ///                      .arg("file.txt")
    ///                      .status()
    ///                      .expect("failed to execute process");
    ///
    /// println!("process exited with: {}", status);
    ///
    /// assert!(status.success());
    /// ```
    #[cfg(feature = "DISABLED")]
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn status(&mut self) -> io::Result<ExitStatus> {
        self.inner.spawn(imp::Stdio::Inherit, true).map(Child::from_inner)
            .and_then(|mut p| p.wait())
    }
}

#[cfg_attr(feature = "ruststd", stable(feature = "rust1", since = "1.0.0"))]
impl fmt::Debug for Command {
    /// Format the program and arguments of a Command for display. Any
    /// non-utf8 data is lossily converted using the utf8 replacement
    /// character.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}


async fn run_async_command_test() -> std::process::Output {
    let mut cmd =
        Command::new("/bin/ls")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .arg("--help")
            .spawn()
            .unwrap();
    cmd.await
}

// ls -1 | sort


#[test]
fn test_run_async_command_test() {
    crate::deps::struthionidae::logging::initialize();

    let mut pool = futures::executor::LocalPool::new();
    let output = pool.run_until(run_async_command_test());
    info!("output: {:?}", output);
}