# STEP FAST

A fast, standalone STEP (.step/.stp) viewer for Windows, written in Rust with OpenGL. STEP import runs in a separate process, and tessellated geometry is cached on disk in a binary format, so reopening a model is quick. No browser, web server or CAD install required.

The user interface is currently in Swedish; the labels referred to below are given with an English translation.

## Download

Grab `StepFast.exe` from the [latest release](../../releases/latest) and run it. The GPU must support OpenGL 3.3.

The release exe uses the built-in Rust importer (Truck). Files Truck can't handle need the optional OpenCascade runtime, see [Import and performance](#import-and-performance).

## Build and run

```powershell
.\native\build.ps1
.\StepFast.exe
# Or open a file directly:
.\StepFast.exe "C:\models\part.step"
```

Building needs:

- Rust stable with the `x86_64-pc-windows-gnu` toolchain (`rustup toolchain install stable-x86_64-pc-windows-gnu`).
- MinGW binutils on PATH (`dlltool` and `as`), for example `winget install BrechtSanders.WinLibs.POSIX.UCRT`.

The script links with Rust's bundled `rust-lld`. The finished exe needs neither Rust nor MinGW.

`.\native\register-windows.ps1` adds a Start menu shortcut and registers .step/.stp files with StepFast for the current user (no admin). Run it again if you move the folder. On Windows 11, choose StepFast and "Always" yourself the first time you open a STEP file.

## Controls

- Open a file with the button, by drag and drop, or as a command-line argument.
- Left-drag rotates, right-drag pans, scroll zooms.
- F fits the model, Home returns to the isometric home view, O opens the file picker.
- With the pointer over the model: Tab hides the body under the pointer, Shift+Tab restores the last hidden body. Repeat to restore more, or use "Visa alla kroppar" (show all bodies). Hidden bodies can't be picked for measuring. A new import shows all bodies again.
- A SpaceMouse is read automatically while the app has focus. Sensitivity starts at 3 and can be set from 0.1 to 8 in the side panel. Zoom direction is inverted by default. The Fit button restores the home view and fits the whole model.
- The 3D mouse uses object mode: panning follows your hand, and pushing forward/back zooms without moving the rotation centre. Under "3D-mus: riktningar" (3D mouse: directions) each axis can be inverted separately. The directions are saved in `%LOCALAPPDATA%\StepFast\navigation-v2.bin`. This is StepFast's own Raw Input navigation, not the SolidWorks/Fusion driver integration.
- M turns on face measuring. Click two faces, rotating between clicks if needed. The faces are highlighted blue and orange, and their minimum distance is shown with a dimension line. A third click starts a new measurement.
- Measurements are approximate and computed on the triangulated faces. Choose "Fin detalj" (fine detail) for better accuracy on curved faces. The unit is shown next to the value.

SpaceMouse support uses Windows Raw Input for 3Dconnexion/Logitech multi-axis devices. Standard wired reports and combined wireless reports are handled. 3DxWare profiles, radial menus and the special buttons are not implemented.

## Grid and live import

On opening a model, an adaptive XY millimetre grid shows the overall X/Y dimensions and a vertical Z-height bracket. Extents come from the triangulated model (approximate for curved surfaces) and include hidden bodies. Rotating more than 5 degrees from the activation orientation fades the grid out over 0.25 seconds. Re-enable it with "Måttgrid" (dimension grid) in the sidebar. Grid spacing is shown at the bottom left; panning and zooming do not dismiss it.

Import reports its current stage and streams completed geometry snapshots, limited to roughly eight updates per second. OpenCascade tessellates one body at a time and publishes surface batches. Truck triangulates in parallel before publishing assembled display geometry. Initial STEP parsing must finish before surfaces are available. Fast imports or cache hits may finish before any partial display; no artificial delay is added. Measuring and hiding are disabled while the model is incomplete. Non-mm STEP models use OpenCascade to normalize to millimetres.

## Import and performance

STEP parsing and tessellation use the Rust library [Truck](https://github.com/ricosjp/truck). Parts are tessellated in parallel. The import process keeps the UI responsive and can be cancelled. Assembly transforms are applied before GPU upload, and geometry is centred in double precision before conversion to GPU floats.

The cache lives in `%LOCALAPPDATA%\StepFast\mesh-v6`. Its key includes the file contents, importer version and detail level. At most eight entries / 512 MiB are kept, and entries over 256 MiB are not cached. Delete the folder to clear it. Reopening still reads and hash-checks the STEP file but skips tessellation. The time in the status bar covers import including process start and GPU upload, not guaranteed time until the image is on screen.

If Truck can't handle the geometry, StepFast falls back to a local OpenCascade (C++) via a small Python helper process, started only when needed. Keep `native/occt-runtime` next to the app in the same folder layout. Create the runtime with `native/setup-occt.ps1` (needs Python 3.12 x64 with pip and internet during setup). No separate Python install is needed at run time. The runtime contains cadquery-ocp and its dependencies and is much larger than the Rust program itself (about 500 MB).

Each imported body gets a colour from a palette that matches the dark blue theme; the file's own material colours are not used. PMI and the assembly tree are not shown. No benchmark against other viewers has been done, so "world's fastest" is unverified.

## Tests

```powershell
.\native\build.ps1 -Test
```

The OCCT live-stream integration check is `native/occt-runtime/python/python.exe native/test_progress.py path/to/model.step`.

## License

MIT, see [LICENSE](LICENSE).

Test file `tests/fixtures/cube.stp` comes from [occt-import-js](https://github.com/kovacsv/occt-import-js/tree/main/test/testfiles/simple-basic-cube) (LGPL-2.1). `cylinder.step` and `assembly.step` come from [resources4truck](https://github.com/ricosjp/resources4truck/tree/master/step). Truck is Apache-2.0. Dependencies are pinned in `native/Cargo.lock`.
