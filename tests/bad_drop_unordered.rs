#[cfg(any(feature = "debug_order",debug_assertions))]
mod test {

    struct A(bool);

    use static_init::{destructor, dynamic};

    #[dynamic(drop=10)]
    static V0: A = unsafe{A(false)};

    #[dynamic(drop=10)]
    static V1: A = unsafe{A(true)};

    impl Drop for A {
        fn drop(&mut self) {
            if self.0 {
                &*V0;
            }
        }
    }


    fn panic_hook(p: &core::panic::PanicInfo<'_>) -> () {
        println!("Panic caught {}", p);
        std::process::exit(0)
    }

    #[destructor(0)]
    unsafe extern "C" fn set_hook() {
        std::panic::set_hook(Box::new(panic_hook));
    }

    #[destructor(30)]
    unsafe extern "C" fn bad_exit() {
        libc::_exit(1)
    }
}

#[test]
fn bad_drop_unordered() {
}
