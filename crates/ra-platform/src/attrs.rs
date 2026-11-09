//! Atributos de tamaño y estado del volumen (RF-SEG-12, RNF-ARQ-28, RF-PAN-01).
//!
//! - [`file_attrs_at`]: `getattrlistat(2)` con los atributos comunes
//!   extendidos (`FSOPT_ATTR_CMN_EXTENDED`): `ATTR_CMNEXT_PRIVATESIZE` (bytes
//!   que se liberarían al borrar: no cuenta lo atrapado en clones o
//!   instantáneas), `ATTR_CMNEXT_EXT_FLAGS` (`EF_MAY_SHARE_BLOCKS`),
//!   `ATTR_CMNEXT_CLONEID` y `ATTR_CMNEXT_CLONE_REFCNT` (05 §1.5.2).
//!   Cada atributo solo vale si el sistema de ficheros lo devuelve
//!   (`ATTR_CMN_RETURNED_ATTRS`); si no, queda en `None` y quien llama usa
//!   `st_blocks × 512`.
//! - [`volume_space`]: `statfs(2)` para capacidad y espacio libre, y
//!   `kCFURLVolumeAvailableCapacityForImportantUsageKey` para la capacidad
//!   disponible para uso importante, la que muestra Finder, que incluye lo
//!   purgable (05 §1.5.3).
//!
//! Todo el `unsafe` de este módulo es FFI con la libc y CoreFoundation.

use std::ffi::{CStr, CString, c_void};
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// `EF_MAY_SHARE_BLOCKS` de `<sys/stat.h>`: puede compartir bloques con otro
/// fichero (un clon).
pub const EF_MAY_SHARE_BLOCKS: u64 = 0x0000_0001;
/// `EF_SHARES_ALL_BLOCKS` de `<sys/stat.h>`: clon completo de otro fichero.
pub const EF_SHARES_ALL_BLOCKS: u64 = 0x0000_0040;

/// `ATTR_CMNEXT_CLONE_REFCNT` de `<sys/attr.h>` (la libc no lo define).
const ATTR_CMNEXT_CLONE_REFCNT: libc::attrgroup_t = 0x0000_1000;

/// Atributos de tamaño y clonado de una entrada. Cada campo es `None` si el
/// sistema de ficheros no lo devuelve.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileAttrs {
    /// Bytes que se liberarían de inmediato al borrar la entrada.
    pub private_size: Option<u64>,
    /// `ATTR_CMNEXT_EXT_FLAGS` (`EF_*`).
    pub ext_flags: Option<u64>,
    /// Identificador del flujo de datos: los clones completos comparten
    /// `clone_id`.
    pub clone_id: Option<u64>,
    /// Número de clones completos que comparten todos sus bloques con este.
    pub clone_refcnt: Option<u32>,
}

impl FileAttrs {
    /// Puede compartir bloques con otro fichero.
    pub fn may_share_blocks(&self) -> bool {
        self.ext_flags
            .is_some_and(|f| f & (EF_MAY_SHARE_BLOCKS | EF_SHARES_ALL_BLOCKS) != 0)
    }
}

/// Búfer de respuesta de `getattrlistat` para los atributos pedidos, con
/// `FSOPT_PACK_INVAL_ATTRS` (todos los huecos presentes, aunque no valgan).
/// El núcleo alinea cada atributo a 4 bytes; los de 8 bytes se leen sin
/// suponer alineación.
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
struct AttrBuf {
    length: u32,
    returned: libc::attribute_set_t,
    // Orden de los bits en `forkattr` (comunes extendidos):
    private_size: libc::off_t, // ATTR_CMNEXT_PRIVATESIZE 0x0008
    clone_id: u64,             // ATTR_CMNEXT_CLONEID     0x0100
    ext_flags: u64,            // ATTR_CMNEXT_EXT_FLAGS   0x0200
    clone_refcnt: u32,         // ATTR_CMNEXT_CLONE_REFCNT 0x1000
}

/// Lee los atributos de tamaño y clonado de `name` dentro de la carpeta
/// abierta `dir`, sin seguir enlaces simbólicos.
#[allow(unsafe_code)]
pub fn file_attrs_at(dir: BorrowedFd<'_>, name: &CStr) -> io::Result<FileAttrs> {
    let mut req = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_RETURNED_ATTRS,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: libc::ATTR_CMNEXT_PRIVATESIZE
            | libc::ATTR_CMNEXT_CLONEID
            | libc::ATTR_CMNEXT_EXT_FLAGS
            | ATTR_CMNEXT_CLONE_REFCNT,
    };
    let mut buf = std::mem::MaybeUninit::<AttrBuf>::zeroed();
    let options =
        libc::FSOPT_NOFOLLOW | libc::FSOPT_ATTR_CMN_EXTENDED | libc::FSOPT_PACK_INVAL_ATTRS;
    // SAFETY: `req` es un `attrlist` válido que vive durante la llamada;
    // `buf` mide `size_of::<AttrBuf>()` bytes, el tamaño que se declara, y el
    // núcleo no escribe más allá; `name` es una cadena C terminada en NUL y
    // `dir` un descriptor abierto prestado durante la llamada.
    let rc = unsafe {
        libc::getattrlistat(
            dir.as_raw_fd(),
            name.as_ptr(),
            (&raw mut req).cast::<c_void>(),
            buf.as_mut_ptr().cast::<c_void>(),
            std::mem::size_of::<AttrBuf>(),
            options as libc::c_ulong,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: el búfer se creó a ceros y todos sus campos son enteros, así
    // que cualquier patrón de bits es un valor válido.
    let buf = unsafe { buf.assume_init() };
    // La disposición del búfer es fija (`FSOPT_PACK_INVAL_ATTRS`): si un SDK
    // futuro la cambiara, aquí se vería.
    debug_assert_eq!(buf.length as usize, std::mem::size_of::<AttrBuf>());
    let returned = buf.returned.forkattr;
    let has = |bit: libc::attrgroup_t| returned & bit != 0;
    // Copias locales: los campos de una estructura `packed` no se pueden
    // referenciar.
    let (private_size, clone_id, ext_flags, clone_refcnt) = (
        buf.private_size,
        buf.clone_id,
        buf.ext_flags,
        buf.clone_refcnt,
    );
    Ok(FileAttrs {
        private_size: has(libc::ATTR_CMNEXT_PRIVATESIZE).then_some(private_size.max(0) as u64),
        ext_flags: has(libc::ATTR_CMNEXT_EXT_FLAGS).then_some(ext_flags),
        clone_id: has(libc::ATTR_CMNEXT_CLONEID).then_some(clone_id),
        clone_refcnt: has(ATTR_CMNEXT_CLONE_REFCNT).then_some(clone_refcnt),
    })
}

/// Espacio del volumen que contiene una ruta, en bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeSpace {
    /// Capacidad total (`f_blocks × f_bsize`).
    pub capacity: u64,
    /// Libre para el usuario (`f_bavail × f_bsize`), sin contar lo purgable.
    pub free: u64,
    /// Disponible para uso importante, como Finder: libre más lo que macOS
    /// puede purgar. `None` si CoreFoundation no lo da.
    pub available_important: Option<u64>,
}

/// Capacidad, espacio libre y disponible del volumen de `path`.
#[allow(unsafe_code)]
pub fn volume_space(path: &Path) -> io::Result<VolumeSpace> {
    let cpath = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL en la ruta"))?;
    let mut st = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `cpath` es una cadena C válida y `st` tiene el tamaño de
    // `struct statfs`, que es lo que la función escribe.
    let rc = unsafe { libc::statfs(cpath.as_ptr(), st.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `statfs` devolvió 0, así que rellenó la estructura.
    let st = unsafe { st.assume_init() };
    let bsize = u64::from(st.f_bsize);
    Ok(VolumeSpace {
        capacity: st.f_blocks.saturating_mul(bsize),
        free: st.f_bavail.saturating_mul(bsize),
        available_important: cf::available_important(path.as_os_str().as_bytes()),
    })
}

/// Llamadas mínimas a CoreFoundation para leer una propiedad numérica de
/// volumen de un `CFURL`.
#[allow(unsafe_code)]
mod cf {
    use std::ffi::c_void;

    type CFTypeRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFURLRef = *const c_void;
    type CFErrorRef = *const c_void;
    type CFIndex = isize;
    type Boolean = u8;
    type CFTypeID = usize;

    /// `kCFNumberSInt64Type`.
    const K_CF_NUMBER_SINT64_TYPE: CFIndex = 4;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFURLVolumeAvailableCapacityForImportantUsageKey: CFStringRef;
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: CFAllocatorRef,
            buffer: *const u8,
            len: CFIndex,
            is_directory: Boolean,
        ) -> CFURLRef;
        fn CFURLCopyResourcePropertyForKey(
            url: CFURLRef,
            key: CFStringRef,
            value: *mut CFTypeRef,
            error: *mut CFErrorRef,
        ) -> Boolean;
        fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
        fn CFNumberGetTypeID() -> CFTypeID;
        fn CFNumberGetValue(number: CFTypeRef, the_type: CFIndex, value: *mut c_void) -> Boolean;
        fn CFRelease(cf: CFTypeRef);
    }

    /// `kCFURLVolumeAvailableCapacityForImportantUsageKey` de la ruta `path`
    /// (bytes del sistema de ficheros, sin NUL final).
    pub(super) fn available_important(path: &[u8]) -> Option<u64> {
        let len = CFIndex::try_from(path.len()).ok()?;
        // SAFETY: `path` es un búfer válido de `len` bytes; con el asignador
        // nulo se usa el predeterminado. Devuelve un objeto propio (regla
        // «Create») o nulo.
        let url = unsafe {
            CFURLCreateFromFileSystemRepresentation(std::ptr::null(), path.as_ptr(), len, 1)
        };
        if url.is_null() {
            return None;
        }
        let mut value: CFTypeRef = std::ptr::null();
        // SAFETY: `url` es un CFURL válido y propio; la clave es una
        // constante de CoreFoundation; `value` recibe un objeto propio (regla
        // «Copy») o queda nulo. El error no se pide (puntero nulo).
        let ok = unsafe {
            CFURLCopyResourcePropertyForKey(
                url,
                kCFURLVolumeAvailableCapacityForImportantUsageKey,
                &mut value,
                std::ptr::null_mut(),
            )
        };
        // SAFETY: `url` es propio y no se vuelve a usar.
        unsafe { CFRelease(url) };
        if ok == 0 || value.is_null() {
            return None;
        }
        let mut out: i64 = 0;
        // SAFETY: `value` es un objeto CF válido; solo se lee como CFNumber
        // si su tipo lo es, y `out` es un `i64`, que es lo que pide
        // `kCFNumberSInt64Type`.
        let got = unsafe {
            CFGetTypeID(value) == CFNumberGetTypeID()
                && CFNumberGetValue(value, K_CF_NUMBER_SINT64_TYPE, (&raw mut out).cast()) != 0
        };
        // SAFETY: `value` es propio y no se vuelve a usar.
        unsafe { CFRelease(value) };
        (got && out >= 0).then_some(out as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::os::fd::AsFd;
    use std::process::Command;

    fn attrs(dir: &Path, name: &str) -> FileAttrs {
        let d = File::open(dir).expect("abrir carpeta");
        let c = CString::new(name).expect("nombre");
        file_attrs_at(d.as_fd(), &c).expect("getattrlistat")
    }

    fn clone_of(dir: &Path, from: &str, to: &str) {
        let st = Command::new("cp")
            .arg("-c")
            .arg(dir.join(from))
            .arg(dir.join(to))
            .status()
            .expect("cp -c");
        assert!(st.success(), "cp -c falló");
    }

    fn allocated(path: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt;
        fs::symlink_metadata(path).expect("lstat").blocks() * 512
    }

    #[test]
    fn lone_file_frees_all_its_blocks() {
        let t = tempfile::tempdir().expect("tempdir");
        fs::write(t.path().join("a"), vec![7u8; 1 << 20]).expect("write");
        let a = attrs(t.path(), "a");
        assert_eq!(a.private_size, Some(allocated(&t.path().join("a"))));
        assert!(!a.may_share_blocks());
        assert_eq!(a.clone_refcnt, Some(1));
    }

    #[test]
    fn full_clones_share_everything() {
        let t = tempfile::tempdir().expect("tempdir");
        fs::write(t.path().join("a"), vec![7u8; 1 << 20]).expect("write");
        clone_of(t.path(), "a", "b");
        let (a, b) = (attrs(t.path(), "a"), attrs(t.path(), "b"));
        for x in [a, b] {
            assert_eq!(x.private_size, Some(0), "{x:?}");
            assert!(x.may_share_blocks(), "{x:?}");
            // El recuento incluye al propio fichero.
            assert_eq!(x.clone_refcnt, Some(2), "{x:?}");
        }
        assert_eq!(a.clone_id, b.clone_id);
    }

    #[test]
    fn symlinks_are_not_followed() {
        let t = tempfile::tempdir().expect("tempdir");
        fs::write(t.path().join("a"), vec![7u8; 1 << 20]).expect("write");
        std::os::unix::fs::symlink("a", t.path().join("s")).expect("symlink");
        let s = attrs(t.path(), "s");
        assert_ne!(s.clone_id, attrs(t.path(), "a").clone_id);
        assert_ne!(s.private_size, Some(1 << 20));
    }

    #[test]
    fn missing_entry_is_an_error() {
        let t = tempfile::tempdir().expect("tempdir");
        let d = File::open(t.path()).expect("abrir carpeta");
        let c = CString::new("nada").expect("nombre");
        assert!(file_attrs_at(d.as_fd(), &c).is_err());
    }

    #[test]
    fn volume_space_is_consistent() {
        let v = volume_space(Path::new("/")).expect("statfs");
        assert!(v.capacity > 0);
        assert!(v.free <= v.capacity, "{v:?}");
        // `available_important` depende del entorno (en un sandbox sale 0):
        // solo se comprueba que no supera la capacidad.
        assert!(
            v.available_important.is_none_or(|a| a <= v.capacity),
            "{v:?}"
        );
    }

    /// Criterio de T-05: «libre» coincide con `diskutil info /` (± 1 %).
    /// Se ignora por defecto porque `diskutil` necesita DiskArbitration, que
    /// no está en el sandbox ni en todos los runners:
    /// `cargo test -p ra-platform -- --ignored free_matches_diskutil`.
    /// «Disponible» frente a Finder queda en la lista manual de T-29.
    #[test]
    #[ignore = "necesita diskutil (DiskArbitration)"]
    fn free_matches_diskutil() {
        let out = std::process::Command::new("diskutil")
            .args(["info", "-plist", "/"])
            .output()
            .expect("diskutil");
        assert!(out.status.success(), "diskutil: {out:?}");
        let info: plist::Dictionary = plist::from_bytes(&out.stdout).expect("plist");
        let diskutil_free = info
            .get("FreeSpace")
            .or_else(|| info.get("APFSContainerFree"))
            .and_then(plist::Value::as_unsigned_integer)
            .expect("FreeSpace en diskutil");
        let v = volume_space(Path::new("/")).expect("statfs");
        let diff = v.free.abs_diff(diskutil_free) as f64;
        assert!(
            diff <= diskutil_free as f64 * 0.01,
            "statfs {} frente a diskutil {}",
            v.free,
            diskutil_free
        );
    }
}
