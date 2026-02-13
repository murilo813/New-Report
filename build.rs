extern crate winres;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        
        res.set_icon("icon.ico");
        
        res.set("ProductName", "New Report");
        res.set("FileDescription", "Motor de Relatórios SQL para bases DBISAM");
        res.set("LegalCopyright", "Copyright (c) 2026 Murilo de Souza");
        res.set("CompanyName", "Murilo de Souza");
        res.set("OriginalFilename", "NewReport.exe");

        res.set_manifest(r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
        <requestedPrivileges>
            <requestedExecutionLevel level="asInvoker" uiAccess="false" />
        </requestedPrivileges>
    </security>
</trustInfo>
<compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
        <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" />
    </application>
</compatibility>
</assembly>
"#);

        res.compile().unwrap();
    }
}