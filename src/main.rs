// Copyright © 2015, Peter Atashian
// Licensed under the MIT License <LICENSE.md>
#![feature(collections, env, io, process, std_misc)]

extern crate "winapi" as win;
extern crate "kernel32-sys" as k32;

use std::env::{args, current_dir, set_exit_status};
use std::ffi::{AsOsStr};
use std::io::{Error};
use std::mem::{size_of_val, zeroed};
use std::os::windows::prelude::*;
use std::process::{Command};
use std::ptr::{null_mut};

static mut job_handle: win::HANDLE = 0 as win::HANDLE;

extern "system" fn handler(_: win::DWORD) -> win::BOOL {
    // If we get any sort of signal, just kill the job object
    assert!(unsafe { k32::TerminateJobObject(job_handle, 273) } != 0,
        "Failed to terminate job object: {}", Error::last_os_error());
    win::TRUE
}
fn main() {
    // We use the current directory as the job object name
    // This way we can clean up job objects lingering from previous builds
    let dir = current_dir().unwrap();
    let name: Vec<_> = dir.as_os_str().encode_wide().collect();
    // Replace \ with /, otherwise it interprets the name as a path
    let name = name.map_in_place(|c| if c == '\\' as u16 { '/' as u16 } else { c });
    let mut handle = unsafe { k32::CreateJobObjectW(null_mut(), name.as_ptr()) };
    assert!(handle != null_mut(), "Failed to create job object: {}", Error::last_os_error());
    if unsafe { k32::GetLastError() } == win::ERROR_ALREADY_EXISTS {
        // If there is a previous job object, kill it and then create a new one
        assert!(unsafe { k32::TerminateJobObject(handle, 9001) } != 0,
            "Failed to terminate existing job object: {}", Error::last_os_error());
        // We have to sleep otherwise this process might get killed in the above termination
        unsafe { k32::Sleep(1000) };
        handle = unsafe { k32::CreateJobObjectW(null_mut(), name.as_ptr()) };
        assert!(handle != null_mut(), "Failed to recreate job object: {}", Error::last_os_error());
    }
    unsafe { job_handle = handle };
    // Automatically terminate job object when all handles to it close
    let mut info: win::JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    info.BasicLimitInformation.LimitFlags = win::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    assert!(unsafe { k32::SetInformationJobObject(
            handle, win::JOBOBJECTINFOCLASS::JobObjectExtendedLimitInformation,
            &mut info as *mut _ as win::LPVOID, size_of_val(&info) as win::DWORD) } != 0,
        "Failed to set job object limit information: {}", Error::last_os_error());
    // Everything created from this process will be part of the job object
    let process = unsafe { k32::GetCurrentProcess() };
    assert!(unsafe { k32::AssignProcessToJobObject(handle, process) } != 0,
        "Failed to assign process to job object: {}", Error::last_os_error());
    // Set up a signal handler to kill the job object if things go south
    assert!(unsafe { k32::SetConsoleCtrlHandler(handler, win::TRUE) } != 0,
        "Failed to set signal handler: {}", Error::last_os_error());
    let args: Vec<_> = args().collect();
    // Actually spawn the command you really wanted
    let status = Command::new(&args[1]).args(&args[2..]).status().unwrap();
    set_exit_status(status.code().unwrap())
}
