// Copyright 2021 Olivier Kannengieser
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![cfg_attr(not(any(feature = "lazy", feature = "thread_local_drop")), no_std)]
#![cfg_attr(all(elf), feature(linkage))]
#![feature(thread_local)]
#![feature(cfg_target_thread_local)]
//! Non const static initialization, and program constructor/destructor code.
//!
//! # Lesser Lazy Statics
//!
//! This crate provides *lazy statics* on all plateforms.
//!
//! On unixes and windows *lesser lazy statics* are *lazy* during program startup phase
//! (before `main` is called). Once main is called, those statics are all guaranteed to be
//! initialized and any access to them is as fast as any access to regular const initialized
//! statics. Benches sho that usual lazy statics, as those provided by `std::lazy::*` or from
//! [lazy_static][1] crate, suffer from a 2ns access penalty.
//!
//! *Lesser lazy statics* can optionaly be dropped at program destruction
//! (after main exit but before the program stops).
//!
//! *Lesser lazy statics* require the standard library and are enabled by default
//! crate features `lazy` and `atexit`.
//! ```rust
//! use static_init::{dynamic};
//!
//! #[dynamic] //equivalent to #[dynamic(lazy)]
//! static L1: Vec<i32> = unsafe{L0.clone()};
//!
//! #[dynamic(drop)] //equivalent to #[dynamic(lazy,drop)]
//! static L0: Vec<i32> = vec![1,2,3];
//!
//! #[dynamic(drop)]
//! static mut L2: Vec<i32> = L1.clone();
//! #
//! # assert_eq!(L1[0], 1);
//! # unsafe {
//! #     assert_eq!(L2[1], 2);
//! #     L2[1] = 42;
//! #     assert_eq!(L2[1], 42);
//! #     }
//! #     
//! ```
//! As can be seen above accesses to *lazy static* that are dropped must be within unsafe
//! blocks. The reason is that it is possible at program destruction to access already dropped
//! lazy statics.
//!
//! # Dynamic statics: statics initialized at program startup
//!
//! On plateforms that support it (unixes, mac, windows), this crate provides *dynamic statics*: statics that are
//! initialized at program startup. This feature is `no_std`.
//!
//! ```rust
//! use static_init::{dynamic};
//!
//! #[dynamic(0)]
//! //equivalent to #[dynamic(init=0)]
//! static D1: Vec<i32> = vec![1,2,3];
//!
//! assert_eq!(unsafe{D1[0]}, 1);
//! ```
//! As can be seen above, even if D1 is not mutable, access to it must be performed in unsafe
//! blocks. The reason is that during startup phase, accesses to *dynamic statics* may cause
//! *undefined behavior*: *dynamic statics* may be in a zero initialized state.
//!
//! To prevent such hazardeous accesses, on unixes and window plateforms, a priority can be
//! specified. Dynamic static initializations with higher priority are sequenced before dynamic
//! static initializations with lower priority. Dynamic static initializations with the same
//! priority are underterminately sequenced.
//!
//! ```rust
//! use static_init::{dynamic};
//!
//! // D2 initialization is sequenced before D1 initialization
//! #[dynamic(0)]
//! static mut D1: Vec<i32> = unsafe{D2.clone()};
//!
//! #[dynamic(10)]
//! static D2: Vec<i32> = vec![1,2,3];
//! #
//! # unsafe{assert_eq!(D1[0], 1)};
//! ```
//!
//! *Dynamic statics* can be dropped at program destruction phase: they are dropped after main
//! exit:
//!
//! ```rust
//! use static_init::{dynamic};
//!
//! // D2 initialization is sequenced before D1 initialization
//! // D1 drop is sequenced before D2 drop.
//! #[dynamic(init=0,drop=0)]
//! static mut D1: Vec<i32> = unsafe {D2.clone()};
//!
//! #[dynamic(init=10,drop=10)]
//! static D2: Vec<i32> = vec![1,2,3];
//! ```
//! The priority act on drop in reverse order. *Dynamic statics* drops with a lower priority are
//! sequenced before *dynamic statics* drops with higher priority.
//!
//! Finally, if the feature `atexit` is enabled, *dynamic statics* drop can be registered with
//! `libc::atexit`. *lazy dynamic statics* and *dynamic statics* with `drop_reverse` attribute
//! argument are destroyed in the reverse order of their construction. Functions registered with
//! `atexit` are executed before program destructors and drop of *dynamic statics* that use the
//! `drop` attribute argument. Drop is registered with at `atexit` if no priority if given to the
//! `drop` attribute argument.
//!
//! ```rust
//! use static_init::{dynamic};
//!
//! //D1 is dropped before D2 because
//! //it is initialized before D2
//! #[dynamic(lazy,drop)]
//! static D1: Vec<i32> = vec![0,1,2];
//!
//! #[dynamic(10,drop)]
//! static D2: Vec<i32> = unsafe{D1.clone()};
//!
//! //D3 is initilized after D1 and D2 initializations
//! //and it is dropped after D1 and D2 drops
//! #[dynamic(5,drop)]
//! static D3: Vec<i32> = unsafe{D1.clone()};
//! ```
//!
//! # Constructor and Destructor
//!
//! On plateforms that support it (unixes, mac, windows), this crate provides a way to declare
//! *constructors*: a function called before main is called. This feature is `no_std`.
//!
//! ```rust
//! use static_init::{constructor};
//!
//! //called before main
//! #[constructor] //equivalent to #[constructor(0)]
//! extern "C" fn some_init() {}
//! ```
//!
//! Constructors also support priorities. Sequencement rules applies also between constructor calls and
//! between *dynamic statics* initialization and *constructor* calls.
//!
//! *destructors* are called at program destruction. They also support priorities.
//!
//! ```rust
//! use static_init::{constructor, destructor};
//!
//! //called before some_init
//! #[constructor(10)]
//! extern "C" fn pre_init() {}
//!
//! //called before main
//! #[constructor]
//! extern "C" fn some_init() {}
//!
//! //called after main
//! #[destructor]
//! extern "C" fn first_destructor() {}
//!
//! //called after first_destructor
//! #[destructor(10)]
//! extern "C" fn last_destructor() {}
//! ```
//!
//! # Thread Local Support
//!
//! Variable declared with `#[dynamic(lazy)]` can also be declared `#[thread_local]`. These
//! variable will behave as regular *lazy statics*.
//! ```ignore
//! #[thread_local]
//! #[dynamic(lazy)]
//! static mut X: Vec<i32> = vec![1,2,3];
//! ```
//! These variables can also be droped on thread exit.
//! ```ignore
//! #[thread_local]
//! #[dynamic(lazy,drop)]
//! static X: Vec<i32> = vec![1,2,3];
//!
//! assert!(unsafe{X[1] == 2});
//! ```
//!
//! Accessing a thread local *lazy statics* that should drop during the phase where thread_locals are
//! droped may cause *undefined behavior*. For this reason any access to a thread local lazy static
//! that is dropped will require an unsafe block, even if the static is const.
//!
//!
//! # Debuging initialization order
//!
//! If the feature `debug_order` is enabled, attempts to access `dynamic statics` that are
//! uninitialized or whose initialization is undeterminately sequenced with the access will cause
//! a panic with a message specifying which statics was tentatively accessed and how to change this
//! *dynamic static* priority to fix this issue.
//!
//! Run `cargo test` in this crate directory to see message examples.
//!
//! All implementations of lazy statics may suffer from circular initialization dependencies. Those
//! circular dependencies will cause either a dead lock or an infinite loop. If the feature `debug_order` is
//! enabled, atemp are made to detect those circular dependencies. In most case they will be detected.
//!
//! [1]: https://crates.io/crates/lazy_static

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
// # Potential usages:
//
// ## Initialization of a runtime as a static object
//
// Pros:
//
//   - Access to runtime is done through the static object which
//   simplify program code: there is no need to pass the runtime
//   as an argument.
//
// Cons with Lazy:
//
//   - Slow down due to recurring check to see weither the static is initialized or not
//
//   - Lazy may lead to cycles which will lock the program, but that can be detected in debug mode
//
// Cons with QuasiLazy:
//
//   - Non optional initialization of the runtime so it is better not used in a library
//   or only as a opt-in feature.
//
//   - Fall back to Lazy on mach and plateforms that are not unixes or windows
//
//   - Same trouble with cycles
//
// Cons with Dynamic Static:
//
//   - Undefined behavior if combined with other dynamic statics that try to access it: it
//   should only be used in final executable crate: but this can be detected in debug mode
//
//   - Only on windows
//
// Pros with const initialized lazy:
//
//   - the object is const initialized but uses a initialization function which can fails
//   so that the object works in degradated mod.
//
//   - it should be possible in this case to try again initialization
//
//   - or to know if it has succeeded or not (but leave this as an option)
//
// Pros with const initialized lazy:
//
//   - Any access time cost may actualy be forwarded to
//   of the runtime static object if it must adapt its behavior
//   on its initialization state. But for the case that interest me
//   the object may but itself in uninitialized state during normal
//   execution so there are no cost of such const initialization. For
//   this object the const initialization consist in registrating the destructor.
//   On the other hand, trial to check this registration on each access is unusefull
//   and could be done when the object detect itself it is in an uninitialized state.
//
//   More over the object needs to know if registration not even initialization cause
//   a recursion and it is perfectly able to avoid dead lock in this case as it fallback
//   to a degradated mode.
//
//
//
//
// On the usage of drop:
//
//   - drop may be used to for logging, releasing resources etc.
//
//   - it is guaranteed to be executed once, this is what is interesting
//
//   - it potentitialy leave the object in an invalid state, this is obsolutely
//   unusefull and only leads to UB.
//
//   - So either drop is used for finalization and any access to the object after
//   that finalization should lead to panic => runtime cost for checking object
//   state
//
//   - Or we use a more invariant friendly drop => Finaly trait that takes a const
//   object.
//
// ## Initialization of a runtime as a thread local object
//
// A thread local runtime is interesting in the case where access to the runtime object
// may needs synchronization. In this case some of the synchronization primitive may be
// avoided.
//
// As it is not possible to declare an initialization at thread start up (though it could
// be implemented in the standard library quite easily for windows and unixes)
//
// The initialized runtime thread local object may want to release resources at thread exit
// so it needs a finalization phase.
//
// The actual thread_local! implementation in the library may leads to resources leak as
// thread_local object destructor may not be called.
//
// So the crate provides a thread_local implementation that is safer that the one of the standard
// library:
//
//   - registration success of a finalization steps guarantee it will be run.
//
//   - registration failure is reported
//
// What about const initialized const finalized in may case:
//   - actuellement l'objet détecte si il est dans un état non initialisé et essaie la
//   registration du destructeur dans ce cas. Il enregistre le fait qu'il essaie de s'enregistrer.
//   Si la registration est en cours et qu'il est appelé de façon cyclique il sait comment
//   s'adapter. Donc les accèss cyclique sont parfaitement acceptable et ne devrait pas causer
//   de blocage. Deplus il faut que quand il détecte que la registration forme un cycle, il soit
//   garanti que celle-ci réussise et que le destructeur soir appellé. Deplus une fois que le
//   destructeur est appellé, il ne faut pas qu'il essait de s'enregistrer à nouveau et qu'il
//   fonctionne en mode dégradé.
//
//   Il me faut donc plus de souplesse dans l'éplémentation, un tel objet aurait besoin d'accéder
//   directement le at_exit. Le problème est le cractère unsafe de la registration qui est laissée
//   à l'utilisateur final. Comment faire. Le statut doit être dans les méthodes de l'objet.
//
//   Donc il faut:
//     - Fournir de façon publique l'objet qui assure le management et que l'interface
//     de cette objet permette à l'utilisateur de connaitre lui-même la phase dans laquelle est
//     l'objet.
//     - Le wrapper fournit par le crate accèderai alors à cette objet via un trait que
//     l'utilisateur implémenterai lui-meme. De sorte que les macros de la librairie serait
//     ensuite les seules responsable des phases dangereuses?
//
//  Comment faire dans mon cas: c'est l'objet lui-meme qui demande la registration et il faut
//  fournir cette possibilité à l'utilisateur final de façon safe. Il s'assurer que l'objet
//  ne puisse être construit qu'au travers de la librairie.
//
//  Comment résoud cela la librairie standard: elle déclare l'objet comme un objet normal
//  accessible uniquement de facon statique et dont l'initializer est "caché". Deplus
//  au lieu d'implémenter deref, elle implémente "with" comme une méthode n'acceptant que les objet
//  statiques.
//
//  Le problème est qu'il est impossible de fournir une méthode deref dans ce cas.
//
//  Donc autant déclarer un fonction new as unsafe, pour créer l'objet. La macro ne
//  se prive pas de le créer puisque qu'elle assurera qu'il soit thread_local.

/// Manager and Data should refer to object that are parts of Self structure.
///
/// Moreover, to be usable the type should provide an associated function of signature:
/// `unsafe const fn new_static(<init_expr>, _:Manager) -> Self` which is safe to call as long as
/// the target object is a static.
///
/// The data refered by manager should not be modified by the implementor
/// of the trait (for example through a union).
pub unsafe trait Static: 'static + Sized {
    type Data: 'static;
    type Manager: 'static;
    fn manager(this: &Self) -> &Self::Manager;
    fn data(this: &Self) -> &Self::Data;
}
//Le manager est une struct publique,
//avec une interface qui est safe, mais
//dont la safety n'est assuré que dans la mesure
//ou le manager et donc l'objet qui le contient
//est thread local.
//
//Il faut donc que l'objet propose une fonction new_managed
//qui soit unsafe et const. Et celle si doit produire un objet
//contenant the manager qui est référencé par les appel à `manager`.
//le unsafe const new_managed(<Self as Deref>::Target,Manager)

pub trait Manager<T: Static<Manager = Self>>: 'static + Sized {
    /// return the current phase
    fn phase(&self) -> Phase;
    /// Execute once init and, depending on the manager register <T as Finaly>::finaly
    /// for execution at program exit or thread exit.
    ///
    /// will panic if previous attempt to initialize
    /// leed to a panic
    ///
    /// the `init` function is run before registration
    /// of Finally::finaly for this type target. If it
    /// returns false, registration of finaly is skiped.
    ///
    /// TODO: init is run only if registration of finaly if any
    /// is guaranteed to succeed but registration is not done
    /// if init return false
    fn register(
        s: &T,
        init: impl FnOnce(&<T as Static>::Data) -> bool,
        on_registration_failure: impl FnOnce(&<T as Static>::Data),
    );
}

pub trait Generator<T> {
    fn generate(_: &Self) -> T;
}

impl<U, T: Fn() -> U> Generator<U> for T {
    fn generate(this: &Self) -> U {
        this()
    }
}
pub trait Recoverer<T> {
    fn recover(this: &Self, _: &T);
}

impl<T: Fn(&T)> Recoverer<T> for T {
    fn recover(this: &Self, data: &T) {
        this(data)
    }
}

pub trait Finaly {
    fn finaly(&self);
}

mod details {}

#[doc(inline)]
pub use static_init_macro::constructor;

#[doc(inline)]
pub use static_init_macro::destructor;

#[doc(inline)]
pub use static_init_macro::dynamic;

mod generic_lazy;
pub use generic_lazy::{GenericLazy, RegisterOnFirstAccess, UnInited};

mod once;
pub use once::{GlobalOnce, LocalOnce, PkOnce};
//#[cfg(feature = "lazy")]
//pub use static_lazy::Lazy;

mod at_exit;
pub use at_exit::{AtExit, AtThreadExit, AtGlobalExit, GlobalManager, LocalManager};

pub mod raw_static;

mod phase {
    use core::mem::transmute;
    use core::sync::atomic::{AtomicU8, Ordering};

    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    #[repr(u8)]
    pub enum Phase {
        /// State before initialization phase. Depending on the category
        /// of the static, it may mean the static is in an uninitialized
        /// state, or in the state it has in early phase of program start up
        New                = 0,
        /// The initialization is running
        Initialization     = 1,
        /// For statics that are mean to execut a final staget on program/thread exit
        /// the registration of the final execution is running
        FinalyRegistration = 2,
        /// State of the static after initilization end finaly registration succeed
        Initialized        = 3,
        /// drop_const is being executed     
        FinalyExecution    = 4,
        /// drop_const has been executed     
        Finalized          = 5,
        PostFinalyRegistrationFailure = 6,
        /// drop_const is being executed     
        InitializedWithoutFinaly = 7,
        PostInitializationPanic = 8,
        OnRegistrationFailurePanic = 9,
        InitializedPostOnRegistrationFailure = 10,
        PostFinalyExecutionPanic = 11,
    }

    pub(crate) struct AtomicPhase(AtomicU8);

    impl AtomicPhase {
        pub(crate) const fn new() -> Self {
            Self(AtomicU8::new(0))
        }
        pub(crate) fn get(&self) -> Phase {
            unsafe { transmute(self.0.load(Ordering::Acquire)) }
        }
        pub(crate) fn set(&self, p: Phase) {
            self.0.store(p as u8, Ordering::Release)
        }
    }
}
pub use phase::Phase;

use phase::AtomicPhase;

#[derive(Debug)]
#[doc(hidden)]
pub enum InitMode {
    Const,
    Lazy,
    ProgramConstructor(u16),
}

#[derive(Debug)]
#[doc(hidden)]
pub enum FinalyMode {
    None,
    AtExit,
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

pub struct NullRecoverer;

impl<T> Recoverer<T> for NullRecoverer {
    fn recover(_: &Self, _: &T) {}
}
pub struct FinalyRecoverer;

impl<T: Finaly> Recoverer<T> for FinalyRecoverer {
    fn recover(_: &Self, d: &T) {
        d.finaly()
    }
}

pub type Lazy<T, G> = GenericLazy<T, G, NullRecoverer, PkOnce>;
pub type LocalLazy<T, G> = GenericLazy<T, G, NullRecoverer, LocalOnce>;
pub type GlobalLazy<T, G> = GenericLazy<T, G, NullRecoverer, GlobalOnce>;
pub type LazyFinalize<T, G> = GenericLazy<T, G, FinalyRecoverer, AtExit>;
pub type GlobalLazyFinalize<T, G> = GenericLazy<T, G, FinalyRecoverer, AtGlobalExit>;
pub type LocalLazyFinalize<T, G> = GenericLazy<T, G, FinalyRecoverer, AtThreadExit>;

#[cfg(test)]
mod test_lazy {
    use super::{PkOnce, Lazy, NullRecoverer};
    static _X: Lazy<u32, fn() -> u32> =
        unsafe { Lazy::new_static(|| {println!("runned");22}, NullRecoverer, PkOnce::new()) };
    #[test]
    fn test() {
        assert_eq!(*_X, 22);
    }
}

#[cfg(test)]
mod test_global_lazy {
    use super::{GlobalOnce, GlobalLazy, NullRecoverer};
    static _X: GlobalLazy<u32, fn() -> u32> =
        unsafe { GlobalLazy::new_static(|| {println!("runned");22}, NullRecoverer, GlobalOnce::new()) };
    #[test]
    fn test() {
        assert_eq!(*_X, 22);
    }
}
#[cfg(test)]
mod test_local_lazy {
    use super::{LocalLazy, LocalOnce, NullRecoverer};
    #[thread_local]
    static _X: LocalLazy<u32, fn() -> u32> =
        unsafe { LocalLazy::new_static(|| 22, NullRecoverer, LocalOnce::new()) };
    #[test]
    fn test() {
        assert_eq!(*_X, 22);
    }
}
#[cfg(test)]
mod test_lazy_finalize {
    use super::{AtExit, Finaly, FinalyRecoverer, GlobalManager, LazyFinalize};
    #[derive(Debug)]
    struct A(u32);
    impl Finaly for A {
        fn finaly(&self) {}
    }
    static _X: LazyFinalize<A, fn() -> A> = unsafe {
        LazyFinalize::new_static(|| A(22), FinalyRecoverer, AtExit::new(GlobalManager::new_pk()))
    };
    #[test]
    fn test() {
        assert_eq!(_X.0, 22);
    }
}
#[cfg(test)]
mod test_global_lazy_finalize {
    use super::{AtGlobalExit, Finaly, FinalyRecoverer, GlobalManager, GlobalLazyFinalize};
    #[derive(Debug)]
    struct A(u32);
    impl Finaly for A {
        fn finaly(&self) {}
    }
    static _X: GlobalLazyFinalize<A, fn() -> A> = unsafe {
        GlobalLazyFinalize::new_static(|| A(22), FinalyRecoverer, AtGlobalExit::new(GlobalManager::new()))
    };
    #[test]
    fn test() {
        assert_eq!(_X.0, 22);
    }
}
#[cfg(test)]
mod test_local_lazy_finalize {
    use super::{AtThreadExit, Finaly, FinalyRecoverer, LocalLazyFinalize, LocalManager};
    #[derive(Debug)]
    struct A(u32);
    impl Finaly for A {
        fn finaly(&self) {}
    }
    #[thread_local]
    static _X: LocalLazyFinalize<A, fn() -> A> = unsafe {
        LocalLazyFinalize::new_static(
            || A(22),
            FinalyRecoverer,
            AtThreadExit::new(LocalManager::new()),
        )
    };
    #[test]
    fn test() {
        assert_eq!(_X.0, 22);
    }
}
