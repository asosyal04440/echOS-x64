//! # POSIX Alt Sistemi — Alt Modüller
//!
//! Bu modül, POSIX uyumluluk katmanının alt bileşenlerini birleştirir:
//!
//! - **pipe**: Anonim ve isimli borular (FIFO); tek-yönlü süreçler arası veri kanalı
//! - **semaphore**: POSIX ve System V semaforları; senkronizasyon primitifleri
//! - **msgq**: System V IPC ve POSIX mesaj kuyrukları
//! - **dlopen**: ELF dinamik yükleyici; `dlopen`/`dlsym`/`dlclose` desteği
//!
//! ## Üst Seviye `posix.rs` ile İlişki
//!
//! Syscall dispatch `src/posix.rs` üzerinden gerçekleşir. Bu alt modüller
//! ilgili kernel veri yapılarını ve düşük seviye mantığı içerir.

pub mod pipe;
pub mod semaphore;
pub mod msgq;
pub mod dlopen;

// Re-export commonly used types and constants
pub use pipe::{
    PipeBuffer, Pipe, Fifo, PipeManager, PipeError,
    sys_pipe, sys_mkfifo,
    PIPE_BUF_SIZE, O_RDONLY, O_WRONLY, O_RDWR, O_NONBLOCK,
};
pub use semaphore::{
    SemUnnamed, SemNamed, SemArray, SemManager, SemError,
    sys_semget, sys_semop, sys_semctl,
    SEMVMX, SEMMSL, SEMMNI,
};
pub use msgq::{
    SysvMsgQueue, PosixMsgQueue, MsgQueueManager, MsgError,
    sys_msgget, sys_msgsnd, sys_msgrcv,
    IPC_CREAT, IPC_EXCL, IPC_NOWAIT, IPC_RMID,
    MSGMAX, MSGMNB,
};
pub use dlopen::{DynamicLoader, LoadedLibrary};
