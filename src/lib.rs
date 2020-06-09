//! This is a stand alone crate that contains both the C++ source code of the
//! CaDiCaL incremental SAT solver together with its Rust binding. The C++
//! files are compiled and statically linked during the build process. This
//! crate works on Linux, Apple and Windows.
//! CaDiCaL won first place in the SAT track of the SAT Race 2019 and second
//! overall place. It was written by Armin Biere, and it is available under the
//! MIT license.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr::null_mut;
use std::time::Instant;

extern "C" {
    fn ccadical_signature() -> *const c_char;
    fn ccadical_init() -> *mut c_void;
    fn ccadical_release(ptr: *mut c_void);
    fn ccadical_add(ptr: *mut c_void, lit: c_int);
    fn ccadical_assume(ptr: *mut c_void, lit: c_int);
    fn ccadical_solve(ptr: *mut c_void) -> c_int;
    fn ccadical_val(ptr: *mut c_void, lit: c_int) -> c_int;
    fn ccadical_failed(ptr: *mut c_void, lit: c_int) -> c_int;
    fn ccadical_set_terminate(
        ptr: *mut c_void,
        data: *mut c_void,
        cb: Option<extern "C" fn(*mut c_void) -> c_int>,
    );
}

/// The CaDiCaL incremental SAT solver. The literals are unwrapped positive
/// and negative integers, exactly as in the DIMACS format. The common IPASIR
/// operations are presented in a safe Rust interface.
/// # Examples
/// ```
/// let mut sat: cadical::Solver = Default::default();
/// sat.add_clause([1, 2].iter().copied());
/// assert_eq!(sat.solve_with([-1].iter().copied()), Some(true));
/// assert_eq!(sat.value(1), Some(false));
/// assert_eq!(sat.value(2), Some(true));
/// ```

pub struct Solver<C: Callbacks = Timeout> {
    ptr: *mut c_void,
    state: Option<bool>,
    cb: Option<Box<C>>,
}

impl<C: Callbacks> Solver<C> {
    /// Constructs a new solver instance.
    pub fn new() -> Self {
        let ptr = unsafe { ccadical_init() };
        Self {
            ptr,
            state: None,
            cb: None,
        }
    }

    /// Returns the name and version of the CaDiCaL library.
    pub fn signature(&self) -> &'static str {
        let s = unsafe { CStr::from_ptr(ccadical_signature()) };
        s.to_str().unwrap_or("invalid")
    }

    /// Adds the given clause to the solver. Negated literals are negative
    /// integers, positive literals are positive ones. All literals must be
    /// non-zero and different from `i32::MIN`.
    #[inline]
    pub fn add_clause<I>(&mut self, clause: I)
    where
        I: Iterator<Item = i32>,
    {
        for lit in clause {
            debug_assert!(lit != 0 && lit != i32::MIN);
            unsafe { ccadical_add(self.ptr, lit) };
        }
        unsafe { ccadical_add(self.ptr, 0) };
        self.state = None;
    }

    /// Solves the formula defined by the added clauses. If the formula is
    /// satisfiable, then `Some(true)` is returned. If the formula is
    /// unsatisfiable, then `Some(false)` is returned. If the solver runs out
    /// of resources or was terminated, then `None` is returned.
    pub fn solve(&mut self) -> Option<bool> {
        if let Some(cb) = &mut self.cb {
            cb.as_mut().started();
        }

        let r = unsafe { ccadical_solve(self.ptr) };
        self.state = if r == 10 {
            Some(true)
        } else if r == 20 {
            Some(false)
        } else {
            None
        };
        self.state
    }

    /// Solves the formula defined by the set of clauses under the given
    /// assumptions.
    pub fn solve_with<I>(&mut self, assumptions: I) -> Option<bool>
    where
        I: Iterator<Item = i32>,
    {
        for lit in assumptions {
            debug_assert!(lit != 0 && lit != i32::MIN);
            unsafe { ccadical_assume(self.ptr, lit) };
        }
        self.solve()
    }

    /// Returns the state of the solver as returned by the last call to
    /// `solve` or `solve_with`. The state becomes `None` if a new clause
    /// is added.
    #[inline]
    pub fn state(&self) -> Option<bool> {
        self.state
    }

    /// Returns the value of the given literal in the last solution. The
    /// state of the solver must be `Some(true)`. The returned value is
    /// `None` if the formula is satisfied regardless of the the value of the
    /// literal.
    #[inline]
    pub fn value(&self, lit: i32) -> Option<bool> {
        debug_assert!(self.state == Some(true));
        debug_assert!(lit != 0 && lit != i32::MIN);
        let val = unsafe { ccadical_val(self.ptr, lit) };
        if val == lit {
            Some(true)
        } else if val == -lit {
            Some(false)
        } else {
            None
        }
    }

    /// Checks if the given assumed literal (passed to `solve_with`) was used
    /// in the proof of the unsatisfiability of the formula. The state of the
    /// solver must be `Some(false)`.
    #[inline]
    pub fn failed(&self, lit: i32) -> bool {
        debug_assert!(self.state == Some(false));
        debug_assert!(lit != 0 && lit != i32::MIN);
        let val = unsafe { ccadical_failed(self.ptr, lit) };
        val == 1
    }

    /// Sets the callbacks to be called while the solver is running.
    /// # Examples
    /// ```
    /// let mut sat: cadical::Solver<cadical::Timeout> = Default::default();
    /// sat.add_clause([1, 2].iter().copied());
    /// sat.set_callbacks(Some(cadical::Timeout::new(0.0)));
    /// assert_eq!(sat.solve(), None);
    /// ```
    pub fn set_callbacks(&mut self, cb: Option<C>) {
        if let Some(cb) = cb {
            if let Some(data) = &mut self.cb {
                *data.as_mut() = cb;
            } else {
                self.cb = Some(Box::new(cb));
                let data = self.cb.as_mut().unwrap();
                let data = data.as_mut() as *mut C as *mut c_void;
                unsafe {
                    ccadical_set_terminate(self.ptr, data, Some(Self::terminate_cb));
                }
            }
        } else {
            self.cb = None;
            let data = null_mut() as *mut c_void;
            unsafe {
                ccadical_set_terminate(self.ptr, data, None);
            }
        }
    }

    extern "C" fn terminate_cb(data: *mut c_void) -> c_int {
        let cb = unsafe { &mut *(data as *mut C) };
        cb.terminate() as c_int
    }
}

impl<C: Callbacks> Default for Solver<C> {
    fn default() -> Self {
        Solver::new()
    }
}

impl<C: Callbacks> Drop for Solver<C> {
    fn drop(&mut self) {
        unsafe { ccadical_release(self.ptr) };
    }
}

/// Callbacks trait for finer control.
pub trait Callbacks {
    /// Called when the `solve` method is called.
    fn started(&mut self);

    /// Called by the solver periodically to check if it should terminate.
    fn terminate(&mut self) -> bool;
}

/// Callbacks implementing a simple timeout.
pub struct Timeout {
    pub started: Instant,
    pub timeout: f32,
}

impl Timeout {
    /// Creates a new timeout structure with the given timeout value.
    pub fn new(timeout: f32) -> Self {
        Timeout {
            started: Instant::now(),
            timeout,
        }
    }
}

impl Callbacks for Timeout {
    #[inline(always)]
    fn started(&mut self) {
        self.started = Instant::now();
    }

    #[inline(always)]
    fn terminate(&mut self) -> bool {
        self.started.elapsed().as_secs_f32() >= self.timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solver() {
        let mut sat: Solver = Solver::new();
        assert!(sat.signature().starts_with("cadical-"));
        sat.add_clause([1, 2].iter().copied());
        assert_eq!(sat.solve(), Some(true));
        assert_eq!(sat.solve_with([-1].iter().copied()), Some(true));
        assert_eq!(sat.value(1), Some(false));
        assert_eq!(sat.value(2), Some(true));
        assert_eq!(sat.solve_with([-2].iter().copied()), Some(true));
        assert_eq!(sat.value(1), Some(true));
        assert_eq!(sat.value(2), Some(false));
        assert_eq!(sat.solve_with([-1, -2].iter().copied()), Some(false));
        assert_eq!(sat.failed(-1), true);
        assert_eq!(sat.failed(-2), true);
        sat.add_clause([3, 4].iter().copied());
        assert_eq!(sat.solve_with([-1, -2, -3].iter().copied()), Some(false));
        assert_eq!(sat.failed(-1), true);
        assert_eq!(sat.failed(-2), true);
        assert_eq!(sat.failed(-3), false);
    }

    fn pigeon_hole(num: i32) -> Solver {
        let mut sat: Solver = Solver::new();
        for i in 0..(num + 1) {
            sat.add_clause((0..num).map(|j| 1 + i * num + j));
        }
        for i1 in 0..(num + 1) {
            for i2 in 0..(num + 1) {
                if i1 == i2 {
                    continue;
                }
                for j in 0..num {
                    let l1 = 1 + i1 * num + j;
                    let l2 = 1 + i2 * num + j;
                    sat.add_clause([-l1, -l2].iter().copied())
                }
            }
        }
        sat
    }

    #[test]
    fn terminate() {
        let mut sat = pigeon_hole(9);
        let started = Instant::now();
        sat.set_callbacks(Some(Timeout::new(0.5)));
        assert_eq!(sat.solve(), None);
        let elapsed = started.elapsed().as_secs_f32();
        assert!(0.4 < elapsed && elapsed < 0.6);

        let started = Instant::now();
        sat.set_callbacks(Some(Timeout::new(1.0)));
        assert_eq!(sat.solve(), None);
        let elapsed = started.elapsed().as_secs_f32();
        assert!(0.9 < elapsed && elapsed < 1.1);

        sat.set_callbacks(None);
        assert_eq!(sat.solve(), Some(false));
    }
}
