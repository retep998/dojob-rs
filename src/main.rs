// Copyright © 2015, Peter Atashian
// Licensed under the MIT License <LICENSE.md>
#![feature(collections, exit_status)]

extern crate winapi as win;
extern crate kernel32 as k32;

use std::env::{args, current_dir, set_exit_status};
use std::ffi::{OsString};
use std::io::{Error};
use std::mem::{size_of_val, zeroed};
use std::os::windows::prelude::*;
use std::process::{Command};
use std::ptr::{null_mut};

static mut job_handle: win::HANDLE = 0 as win::HANDLE;

fn print_zombies(handle: win::HANDLE) {
    // If there's more than 0x100 zombies you have problems
    const NPROC: usize = 0x100;
    // The joys of variable length arrays in structs
    #[repr(C)] struct ProcessList(win::JOBOBJECT_BASIC_PROCESS_ID_LIST, [win::ULONG_PTR; NPROC]);
    let mut procs = ProcessList(win::JOBOBJECT_BASIC_PROCESS_ID_LIST {
        NumberOfAssignedProcesses: NPROC as win::DWORD,
        NumberOfProcessIdsInList: 0,
        ProcessIdList: [0; 0],
    }, [0; NPROC]);
    // Fetch the list of processes
    if unsafe { k32::QueryInformationJobObject(
        handle, win::JOBOBJECTINFOCLASS::JobObjectBasicProcessIdList,
        &mut procs as *mut _ as win::LPVOID, size_of_val(&procs) as win::DWORD, null_mut(),
    ) } == 0 {
        if unsafe { k32::GetLastError() } == win::ERROR_MORE_DATA {
            panic!("Failed to query job object: {}", Error::last_os_error());
        } else {
            // Seriously, this should not happen
            println!("WARNING: More than {0} zombies! Only displaying first {0}.", NPROC);
        }
    }
    // Print out the processes
    for i in 0..procs.0.NumberOfProcessIdsInList {
        let procid = procs.1[i as usize] as win::DWORD;
        let process = unsafe {
            k32::OpenProcess(win::PROCESS_QUERY_INFORMATION, win::FALSE, procid)
        };
        assert!(process != null_mut(), "Failed to open process: {}", Error::last_os_error());
        let mut buf = [0; 0x1000];
        let len = unsafe {
            k32::K32GetProcessImageFileNameW(process, buf.as_mut_ptr(), buf.len() as win::DWORD)
        };
        assert!(len != 0, "Failed to get process image name: {}", Error::last_os_error());
        let name = OsString::from_wide(&buf[..len as usize]);
        println!("Zombie process: {:?}", name);
    }
}

// In the event of a signal, print out all zombies and abandon ship
unsafe extern "system" fn handler(_: win::DWORD) -> win::BOOL {
    println!("Received signal!");
    print_zombies(job_handle);
    println!("Terminating!");
    let err = k32::TerminateJobObject(job_handle, 273);
    assert!(err != 0, "Failed to terminate job object: {}", Error::last_os_error());
    win::TRUE
}
fn main() {
    // First we get the args
    let args: Vec<_> = args().collect();
    if args.len() < 2 {
        println!("Please pass the command you want to invoke as arguments to this program");
        set_exit_status(1);
        return
    }
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
        println!("WARNING: Terminating zombie job object!");
        print_zombies(handle);
        let err = unsafe { k32::TerminateJobObject(handle, 9001) };
        assert!(err != 0, "Failed to terminate existing job object: {}", Error::last_os_error());
        // We have to sleep otherwise this process might get killed in the above termination
        unsafe { k32::Sleep(1000) };
        handle = unsafe { k32::CreateJobObjectW(null_mut(), name.as_ptr()) };
        assert!(handle != null_mut(), "Failed to recreate job object: {}", Error::last_os_error());
    }
    unsafe { job_handle = handle };
    // Automatically terminate job object when all handles to it close
    let mut info: win::JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    info.BasicLimitInformation.LimitFlags = win::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let err = unsafe { k32::SetInformationJobObject(
        handle, win::JOBOBJECTINFOCLASS::JobObjectExtendedLimitInformation,
        &mut info as *mut _ as win::LPVOID, size_of_val(&info) as win::DWORD,
    ) };
    assert!(err != 0, "Failed to set job object limit information: {}", Error::last_os_error());
    // Everything created from this process will be part of the job object
    let process = unsafe { k32::GetCurrentProcess() };
    let err = unsafe { k32::AssignProcessToJobObject(handle, process) };
    assert!(err != 0, "Failed to assign process to job object: {}", Error::last_os_error());
    // Set up a signal handler to kill the job object if things go south
    let err = unsafe { k32::SetConsoleCtrlHandler(Some(handler), win::TRUE) };
    assert!(err != 0, "Failed to set signal handler: {}", Error::last_os_error());
    // Actually spawn the command you really wanted
    let status = Command::new(&args[1]).args(&args[2..]).status().unwrap();
    // Just in case we get any zombies even though things passed
    print_zombies(handle);
    set_exit_status(status.code().unwrap())
}
