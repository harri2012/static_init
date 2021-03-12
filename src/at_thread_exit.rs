#![cfg(target_thread_local)]
#![cfg(any(elf,windows,all(unix,feature="libc")))]

use super::ConstDrop;
use core::cell::Cell;
use core::ptr::NonNull;

/// Object of this type contain an accessible `data` member and a field `managed` accessible
/// for const initialization only with [COMPLETE_INIT].
///
/// When the method [register] method is called, it will attempt to register the method
/// [drop_const] so that it will be run at thread exit or when unwinded. If registration
/// reports its success, or if the object is already in Registered state, the *call of `drop_const`
/// is **guaranteed** to happen at thread exit*.
///
/// The type behave similarly to `libc::at_exit` except that the registered function
/// is run at thread exit, the data owned and accessed by the handler is necessarily thread
/// local and registration is unlikely to cause any memory allocation. State needed to ensure
/// at thread local exit functionnality is stored directly within the object.
///
/// Function registered through this type will be called in reverse order. There is no
/// order between those registrations and those performed by C++ thread locals or standard library
/// thread locals declared with `thread_local!` macro.
pub struct AtThreadLocalExit<T> {
    pub data:    T,
    pub managed: AtThreadLocalExitManaged,
}

#[derive(Copy, Clone, Eq, PartialEq)]
/// The status in which may be an object of type [AtThreadLocalExit]
pub enum Status {
    ///drop_const not yet registered
    NonRegistered,
    ///registration is running and is guaranteed to succeed
    Registrating,
    ///drop_const registered and it is guaranteed it run
    Registered,
    ///drop_const is being executed
    Executing,
    ///drop_const has been executed
    Executed,
    ///impossible to register drop_const because registration is closed
    RegistrationClosed,
}

/// An opagque type used to managed "at thread exit" registration
pub struct AtThreadLocalExitManaged {
    status: Cell<Status>,
    next:   Cell<Option<NonNull<dyn OnThreadExit>>>,
}

/// Used to const initialize objects of type [AtThreadLocalExit]
pub const COMPLETE_INIT: AtThreadLocalExitManaged = AtThreadLocalExitManaged {
    status: Cell::new(Status::NonRegistered),
    next:   Cell::new(None),
};


trait OnThreadExit {
    fn execute(&self);
    fn set_next(&self, _: Option<NonNull<dyn OnThreadExit>>);
    fn take_next(&self) -> Option<NonNull<dyn OnThreadExit>>;
}

impl<T: 'static + ConstDrop> AtThreadLocalExit<T> {
    /// Return the current status
    pub fn status(&self) -> Status {
        self.managed.status.get()
    }
    /// Register the current object for call of on_thread_exit at
    /// thread destruction
    ///
    /// # Safety
    ///
    ///  `self` must refer to a thread_local static object of the current thread other
    pub unsafe fn register(&self) -> Result<(), Status> {
        let status = self.managed.status.get();
        if status == Status::NonRegistered {
            if registration_closed() {
                self.managed.status.set(Status::RegistrationClosed);
                Err(Status::RegistrationClosed)
            } else {
                self.managed.status.set(Status::Registrating);
                assert!(register_on_thread_exit((self as &dyn OnThreadExit).into()));
                self.managed.status.set(Status::Registered);
                Ok(())
            }
        } else {
            Err(status)
        }
    }
}

impl<T: ConstDrop> OnThreadExit for AtThreadLocalExit<T> {
    fn execute(&self) {
        //debug_assert!(self.status.get() == Status::Registered);
        self.managed.status.set(Status::Executing);
        self.data.const_drop();
        self.managed.status.set(Status::Executed);
    }
    fn set_next(&self, ptr: Option<NonNull<dyn OnThreadExit>>) {
        self.managed.next.set(ptr);
    }
    fn take_next(&self) -> Option<NonNull<dyn OnThreadExit>> {
        self.managed.next.take()
    }
}


#[cfg(coff_thread_at_exit)]
mod windows {
    use super::OnThreadExit;
    use core::cell::Cell;
    use core::ptr::NonNull;

    #[cfg(target_arch = "x86_64")]
    type Reason = u64;
    #[cfg(target_arch = "i686")]
    type Reason = u32;
    //On thread exit
    //non nul pointers between .CRT$XLA and .CRT$XLZ will be
    //run... => So we could implement thread_local drop without
    //registration...
    #[link_section = ".CRT$XLAZ"] //do this after the standard library
    #[used]
    pub static AT_THEAD_EXIT: extern "system" fn(*mut u8, Reason, *mut u8) = destroy;

    extern "system" fn destroy(_: *mut u8, reason: Reason, _: *mut u8) {
        const DLL_THREAD_DETACH: Reason = 3;
        const DLL_PROCESS_DETACH: Reason = 0;
        if reason == DLL_THREAD_DETACH || reason == DLL_PROCESS_DETACH {
            let mut o_ptr = REGISTER.take();
            while let Some(ptr) = o_ptr {
                let r = unsafe { ptr.as_ref() };
                r.execute();
                o_ptr = r.take_next();
                o_ptr.or_else(|| REGISTER.take());
            }
            DONE.set(true)
        }

        // Copy pasted from: std/src/sys/windows/thread_local_key.rs
        //
        // See comments above for what this is doing. Note that we don't need this
        // trickery on GNU windows, just on MSVC.
        //
        // TODO: better implement it as in libstdc++ implementation of __cxa_thread_atexit?
        unsafe { reference_tls_used() };
        #[cfg(target_env = "msvc")]
        unsafe fn reference_tls_used() {
            extern "C" {
                static _tls_used: u8;
            }
            core::ptr::read_volatile(&_tls_used);
        }
        #[cfg(not(target_env = "msvc"))]
        unsafe fn reference_tls_used() {}
    }

    #[thread_local]
    static REGISTER: Cell<Option<NonNull<dyn OnThreadExit>>> = Cell::new(None);

    #[thread_local]
    static DONE: Cell<bool> = Cell::new(false);

    pub(super) unsafe fn register_on_thread_exit(r: &(dyn OnThreadExit + 'static)) -> bool {
        if DONE.get() {
            false
        } else {
            r.set_next(REGISTER.take());
            REGISTER.set(Some(NonNull::new_unchecked(r as *const _ as *mut _)));
            true
        }
    }

    pub(super) fn registration_closed() -> bool {
        DONE.get()
    }
}

#[cfg(cxa_thread_at_exit)]
mod cxa {
    use super::OnThreadExit;
    use core::cell::Cell;
    use core::ptr::{self, NonNull};
    extern "C" {
        #[linkage = "extern_weak"]
        static __dso_handle: *mut u8;
        #[linkage = "extern_weak"]
        static __cxa_thread_atexit_impl: *const core::ffi::c_void;
    }

    /// Register a function along with a pointer.
    ///
    /// When the thread exit, functions register with this
    /// function will be called in reverse order of their addition
    /// and will take as argument the `data`.
    fn at_thread_exit(f: extern "C" fn(*mut u8), data: *mut u8) {

        type CxaThreadAtExit =
            extern "C" fn(f: extern "C" fn(*mut u8), data: *mut u8, dso_handle: *mut u8);

        unsafe {
            assert!(!__cxa_thread_atexit_impl.is_null()); //
            let at_thread_exit_impl: CxaThreadAtExit =
                core::mem::transmute(__cxa_thread_atexit_impl);
            at_thread_exit_impl(f, data, __dso_handle);
        }
    }

    #[thread_local]
    static REGISTER: Cell<Option<NonNull<dyn OnThreadExit>>> = Cell::new(None);

    #[thread_local]
    static DESTROYING: Cell<bool> = Cell::new(false);

    extern "C" fn execute_destroy(_: *mut u8) {
        DESTROYING.set(true);
        let mut o_ptr = REGISTER.take();
        while let Some(ptr) = o_ptr {
            let r = unsafe { ptr.as_ref() };
            r.execute();
            o_ptr = r.take_next().or_else(|| REGISTER.take());
        }
        DESTROYING.set(false);
    }
    /// #Safety
    /// r must refer to a (thread local) static
    pub(super) unsafe fn register_on_thread_exit(r: &(dyn OnThreadExit + 'static)) -> bool {
        let old = REGISTER.take();
        if let Some(old) = old {
            r.set_next(Some(old));
        } else if !DESTROYING.get() {
            at_thread_exit(execute_destroy, ptr::null_mut())
        }
        REGISTER.set(Some(NonNull::new_unchecked(r as *const _ as *mut _)));
        true
    }

    pub(super) fn registration_closed() -> bool {
        false
    }
}

#[cfg(pthread_thread_at_exit)]
mod pthread {
    use super::OnThreadExit;
    use core::cell::Cell;
    use core::ffi::c_void;
    use core::ptr::NonNull;
    use core::sync::atomic::{AtomicUsize, Ordering};

    use libc::{
        pthread_getspecific, pthread_key_create, pthread_key_delete, pthread_key_t,
        pthread_setspecific,
    };

    //minimum number of time a destructor key may be registered while destructors are run
    const _POSIX_THREAD_DESTRUCTOR_ITERATIONS: usize = 4;

    static DESTRUCTOR_KEY: AtomicUsize = AtomicUsize::new(usize::MAX);

    #[thread_local]
    static ITERATION_COUNT: Cell<usize> = Cell::new(0);

    #[thread_local]
    static REGISTER: Cell<Option<NonNull<dyn OnThreadExit>>> = Cell::new(None);

    extern "C" fn execute_destroy(_: *mut c_void) {

        let mut opt_head = REGISTER.take();
        while let Some(ptr) = opt_head {
            let r = unsafe { ptr.as_ref() };
            r.execute();
            opt_head = r.take_next().or_else(|| REGISTER.take());
        }
    }

    pub(super) unsafe fn register_on_thread_exit(r: &(dyn OnThreadExit + 'static)) -> bool {
        let key = {
            let mut key = DESTRUCTOR_KEY.load(Ordering::Acquire);
            let mut lk = 0;
            while key == usize::MAX {
                //The minimum number of key is 128, we require only one contrarily to
                //what happen in standard library (one per thread local on some targets)
                //on glibc the limit is 1024. So this could definitively fail.
                if pthread_key_create(&mut lk as *mut pthread_key_t, Some(execute_destroy)) == 0 {
                    key = DESTRUCTOR_KEY.load(Ordering::Acquire);
                    if key != usize::MAX {
                        break;
                    } else {
                        return false;
                    }
                }
                if lk as usize == usize::MAX {
                    pthread_key_delete(lk);
                } else {
                    key = match DESTRUCTOR_KEY.compare_exchange(
                        usize::MAX,
                        lk as usize,
                        Ordering::Release,
                        Ordering::Acquire,
                    ) {
                        Ok(k) => k,
                        Err(k) => {
                            pthread_key_delete(lk);
                            k
                        }
                    };
                }
            }
            key as pthread_key_t
        };

        let specific = pthread_getspecific(key);

        if specific.is_null() {
            if ITERATION_COUNT.get() < _POSIX_THREAD_DESTRUCTOR_ITERATIONS {
                if pthread_setspecific(key, NonNull::dangling().as_ptr()) == 0 {
                    return false;
                }
                ITERATION_COUNT.set(ITERATION_COUNT.get() + 1);
            } else {
                //it is not guaranted by posix that destructor will be run
                //so refuse registration
                return false;
            }
        }

        r.set_next(REGISTER.take());

        REGISTER.set(Some(NonNull::new_unchecked(r as *const _ as *mut _)));

        true
    }
    pub(crate) fn registration_closed() -> bool {
        ITERATION_COUNT.get() == _POSIX_THREAD_DESTRUCTOR_ITERATIONS
            && unsafe {
                pthread_getspecific(DESTRUCTOR_KEY.load(Ordering::Acquire) as pthread_key_t)
            }
            .is_null()
    }
}
// For mach this is impossible and contrarily to what is done
// in the standard library we use pthread_keys.
// Notice that standard library implementation is bugged:
// variables declared with threak_local!{} on Apple target
// may cause UB if interoperating with a library written in C++ that
// also use thread_locals.

#[cfg(not(any(cxa_thread_at_exit,pthread_thread_at_exit,coff_thread_at_exit)))]
mod fall_back {
    
    pub(super) unsafe fn register_on_thread_exit(r: &(dyn OnThreadExit + 'static)) -> bool {
        false
    }

    pub(crate) fn registration_closed() -> bool {
        true
    }
}

#[cfg(cxa_thread_at_exit)]
use cxa::{register_on_thread_exit, registration_closed};

#[cfg(coff_thread_at_exit)]
use windows::{register_on_thread_exit, registration_closed};

#[cfg(pthread_thread_at_exit)]
use pthread::{register_on_thread_exit, registration_closed};

#[cfg(not(any(cxa_thread_at_exit,pthread_thread_at_exit,coff_thread_at_exit)))]
use fall_back::{register_on_thread_exit, registration_closed};

