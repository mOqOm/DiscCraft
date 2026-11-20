//! The program file's icon on Windows (what Explorer and a pinned taskbar button show): assets/app-icon/disccraft.ico
//! as the executable's icon resource. Elsewhere nothing is done.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/app-icon/disccraft.ico");
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        let _ = resource.set_icon("../../assets/app-icon/disccraft.ico");
        // A machine without the Windows SDK's resource compiler still builds, without the icon.
        if let Err(error) = resource.compile() {
            println!("cargo:warning=the program file gets no icon: {error}");
        }
    }
}
