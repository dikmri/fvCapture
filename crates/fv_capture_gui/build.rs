fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/icons/fvCapture.ico");
        if let Err(error) = resource.compile() {
            panic!("failed to compile Windows resources: {error}");
        }
    }
}
