// Copyright 2021 Olivier Kannengieser
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![cfg_attr(
    not(any(feature = "global_once", feature = "thread_local_drop")),
    no_std
)]
#![cfg_attr(all(elf, feature = "thread_local"), feature(linkage))]
#![cfg_attr(
    feature = "thread_local",
    feature(thread_local),
    feature(cfg_target_thread_local)
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[doc(hidden)]
/// # Details and implementation documentation.
///
/// ## Mac
///   - [MACH_O specification](https://www.cnblogs.com/sunkang/archive/2011/05/24/2055635.html)
///   - GCC source code gcc/config/darwin.c indicates that priorities are not supported.
///
///   Initialization functions pointers are placed in section "__DATA,__mod_init_func" and
///   "__DATA,__mod_term_func"
///
///   std::env is not initialized in any constructor.
///
/// ## ELF plateforms:
///  - `info ld`
///  - linker script: `ld --verbose`
///  - [ELF specification](https://docs.oracle.com/cd/E23824_01/html/819-0690/chapter7-1.html#scrolltoc)
///
///  The runtime will run fonctions pointers of section ".init_array" at startup and function
///  pointers in ".fini_array" at program exit. The linker place in the target object file
///  sectio .init_array all sections from the source objects whose name is of the form
///  .init_array.NNNNN in lexicographical order then the .init_array sections of those same source
///  objects. It does equivalently with .fini_array and .fini_array.NNNN sections.
///
///  Usage can be seen in gcc source gcc/config/pru.c
///
///  Resources of libstdc++ are initialized with priority 65535-100 (see gcc source libstdc++-v3/c++17/default_resource.h)
///  The rust standard library function that capture the environment and executable arguments is
///  executed at priority 65535-99 on gnu platform variants. On other elf plateform they are not accessbile in any constructors. Nevertheless
///  one can read into /proc/self directory to retrieve the command line.
///  Some callbacks constructors and destructors with priority 65535 are
///  registered by rust/rtlibrary.
///  Static C++ objects are usually initialized with no priority (TBC). lib-c resources are
///  initialized by the C-runtime before any function in the init_array (whatever the priority) are executed.
///
/// ## Windows
///
///   std::env is initialized before any constructors.
///
///  - [this blog post](https://www.cnblogs.com/sunkang/archive/2011/05/24/2055635.html)
///
///  At start up, any functions pointer between sections ".CRT$XIA" and ".CRT$XIZ"
///  and then any functions between ".CRT$XCA" and ".CRT$XCZ". It happens that the C library
///  initialization functions pointer are placed in ".CRT$XIU" and C++ statics functions initialization
///  pointers are placed in ".CRT$XCU". At program finish the pointers between sections
///  ".CRT$XPA" and ".CRT$XPZ" are run first then those between ".CRT$XTA" and ".CRT$XTZ".
///
///  Some reverse engineering was necessary to find out a way to implement
///  constructor/destructor priority.
///
///  Contrarily to what is reported in this blog post, msvc linker
///  only performs a lexicographicall ordering of section whose name
///  is of the form "\<prefix\>$\<suffix\>" and have the same \<prefix\>.
///  For example "RUST$01" and "RUST$02" will be ordered but those two
///  sections will not be ordered with "RHUM" section.
///
///  Moreover, it seems that section name of the form \<prefix\>$\<suffix\> are
///  not limited to 8 characters.
///
///  So static initialization function pointers are placed in section ".CRT$XCU" and
///  those with a priority `p` in `format!(".CRT$XCTZ{:05}",65535-p)`. Destructors without priority
///  are placed in ".CRT$XPU" and those with a priority in `format!(".CRT$XPTZ{:05}",65535-p)`.
///
mod details {}

/// A trait for objects that are intinded to transition between phasis.
///
/// The trait is ensafe because the implementor must ensure:
///
/// - the value returned by sequentializer refer to a memory
///    that as the same lifetime as the data and
///
/// - the sequentializer object returned shall be only returned for
/// the "self" object.
///
/// It is thus safe to implement this trait if sequentializer and
/// data refer to different field of the same object.
pub unsafe trait Sequential {
    type Data;
    type Sequentializer;
    fn sequentializer(this: &Self) -> &Self::Sequentializer;
    fn data(this: &Self) -> &Self::Data;
}

/// Trait for objects that know in which phase they are
pub trait Phased {
    /// return the current phase
    fn phase(this: &Self) -> Phase;
}

impl<T> Phased for T
where
    T: Sequential,
    T::Sequentializer: Phased,
{
    fn phase(this: &Self) -> Phase {
        Phased::phase(Sequential::sequentializer(this))
    }
}

/// A [Sequentializer] ensure sequential phase transition of the object it sequentialize
pub trait Sequentializer<T: Sequential<Sequentializer = Self>>: 'static + Sized + Phased {
    /// When called on the Sequential object, it will ensure that the phase transition
    /// in order.
    ///
    /// Decition to perform transition is conditionned by the shall_proceed funciton and
    /// init_on_reg_failure boolean. The init function is intended to be the function that
    /// transition the object to the initialized Phase.
    fn init(
        s: &T,
        shall_proceed: impl Fn(Phase) -> bool,
        init: impl FnOnce(&<T as Sequential>::Data),
        init_on_reg_failure: bool,
    ) -> bool;
}
/// A [SplitedSequentializer] ensure two sequences of sequencial phase transtion: init and finalize
trait SplitedSequentializer<T: Sequential>: 'static + Sized + Phased {
    /// When called on the Sequential object, it will ensure that the phase transition
    /// in order.
    ///
    /// Decition to perform transition is conditionned by the shall_proceed funciton and
    /// init_on_reg_failure boolean. The init function is intended to be the function that
    /// transition the object to the initialized Phase.
    ///
    /// The reg argument is supposed to store the `finalize_callback` method as a callback that will
    /// be run latter during program execution.
    fn init(
        s: &T,
        shall_proceed: impl Fn(Phase) -> bool,
        init: impl FnOnce(&<T as Sequential>::Data),
        reg: impl FnOnce(&T) -> bool,
        init_on_reg_failure: bool,
    ) -> bool;
    /// A callback that is intened to be stored by the `reg` argument of `init` method.
    fn finalize_callback(s: &T, f: impl FnOnce(&T::Data));
}

/// Generates a value of type `T`
pub trait Generator<T> {
    fn generate(&self) -> T;
}

impl<U, T: Fn() -> U> Generator<U> for T {
    fn generate(&self) -> U {
        self()
    }
}

/// A Drop replacement that does not change the state of the object
pub trait Finaly {
    fn finaly(&self);
}

#[cfg_attr(docsrs, doc(cfg(debug_mode)))]
#[cfg(debug_mode)]
#[doc(hidden)]
#[derive(Debug)]
/// Used to passe errors
pub struct CyclicPanic;

/// phases and bits to manipulate them;
pub mod phase {
    pub const INITED_BIT: u32 = 1;
    pub const INITIALIZING_BIT: u32 = 2 * INITED_BIT;
    pub const INITIALIZING_PANICKED_BIT: u32 = 2 * INITIALIZING_BIT;
    pub const INIT_SKIPED_BIT: u32 = 2 * INITIALIZING_PANICKED_BIT;
    pub const LOCKED_BIT: u32 = 2 * INIT_SKIPED_BIT;
    pub const PARKED_BIT: u32 = 2 * LOCKED_BIT;
    pub const REGISTRATING_BIT: u32 = 2 * PARKED_BIT;
    pub const REGISTRATING_PANIC_BIT: u32 = 2 * REGISTRATING_BIT;
    pub const REGISTRATION_REFUSED_BIT: u32 = 2 * REGISTRATING_PANIC_BIT;
    pub const REGISTERED_BIT: u32 = 2 * REGISTRATION_REFUSED_BIT;
    pub const FINALIZING_BIT: u32 = 2 * REGISTERED_BIT;
    pub const FINALIZED_BIT: u32 = 2 * FINALIZING_BIT;
    pub const FINALIZATION_PANIC_BIT: u32 = 2 * FINALIZED_BIT;

    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    /// Phases of a Sequential
    pub struct Phase(pub u32);

    impl Phase {
        pub const fn new() -> Self {
            Self(0)
        }

        pub fn initial_state(self) -> bool {
            self.0 == 0
        }

        pub fn finalize_registration(self) -> bool {
            self.0 & REGISTRATING_BIT != 0
        }
        pub fn finalize_registration_panicked(self) -> bool {
            self.0 & REGISTRATING_PANIC_BIT != 0
        }
        pub fn finalize_registration_refused(self) -> bool {
            self.0 & REGISTRATION_REFUSED_BIT != 0
        }
        pub fn finalize_registration_failed(self) -> bool {
            self.0 & (REGISTRATION_REFUSED_BIT | REGISTRATING_PANIC_BIT) != 0
        }
        pub fn finalize_registrated(self) -> bool {
            self.0 & REGISTERED_BIT != 0
        }

        pub fn initialization(self) -> bool {
            self.0 & INITIALIZING_BIT != 0
        }
        pub fn initialization_panicked(self) -> bool {
            self.0 & INITIALIZING_PANICKED_BIT != 0
        }
        pub fn initialization_skiped(self) -> bool {
            self.0 & INIT_SKIPED_BIT != 0
        }
        pub fn initialized(self) -> bool {
            self.0 & INITED_BIT != 0
        }

        pub fn finalizing(self) -> bool {
            self.0 & FINALIZING_BIT != 0
        }
        pub fn finalization_panic(self) -> bool {
            self.0 & FINALIZATION_PANIC_BIT != 0
        }
        pub fn finalized(self) -> bool {
            self.0 & FINALIZED_BIT != 0
        }
    }
}
#[doc(inline)]
pub use phase::Phase;

#[doc(inline)]
pub use static_init_macro::constructor;

#[doc(inline)]
pub use static_init_macro::destructor;

#[doc(inline)]
pub use static_init_macro::dynamic;

/// Provides policy types for implementation of various lazily initialized types.
pub mod generic_lazy;

/// Provides two sequentializer, one that is Sync, and the other that is not Sync.
pub mod splited_sequentializer;

#[cfg(any(elf, mach_o, coff))]
/// Provides functionnality to execute callback at process/thread exit and sequentializer using
/// those events.
pub mod at_exit;

/// Provides various implementation of lazily initialized types
pub mod lazy;
#[cfg(feature = "global_once")]
#[doc(inline)]
pub use lazy::Lazy;
#[doc(inline)]
pub use lazy::UnSyncLazy;

#[cfg(any(elf, mach_o, coff))]
/// Provides types for statics that are meant to run code before main start or after it exit.
pub mod raw_static;

mod futex;
mod spinwait;

#[derive(Debug)]
#[doc(hidden)]
pub enum InitMode {
    Const,
    Lazy,
    QuasiLazy,
    ProgramConstructor(u16),
}

#[derive(Debug)]
#[doc(hidden)]
pub enum FinalyMode {
    None,
    Drop,
    Finalize,
    ProgramDestructor(u16),
}

#[derive(Debug)]
#[doc(hidden)]
pub struct StaticInfo {
    pub variable_name: &'static str,
    pub file_name:     &'static str,
    pub line:          u32,
    pub column:        u32,
    pub init_mode:     InitMode,
    pub drop_mode:     FinalyMode,
}
