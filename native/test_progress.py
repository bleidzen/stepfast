"""Integration check for real OCCT surface streaming. Usage: python test_progress.py input.step"""
from pathlib import Path
import os, subprocess, sys, time, struct

root=Path(__file__).parent
output=root/'target/progress-test.mesh'
preview=output.with_suffix('.preview')
env=dict(os.environ,STEP_FAST_PROGRESS=str(preview.resolve()))
def counts(path):
    data=path.read_bytes()
    vertices=struct.unpack_from('<Q',data)[0]
    radius,parts,cached,faces=struct.unpack_from('<fQBQ',data,8+vertices*4)
    return vertices//18,parts,faces
try:
    process=subprocess.Popen([str(root/'occt-runtime/python/python.exe'),str(root/'occt-runtime/import_step.py'),sys.argv[1],str(output),'fast'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,creationflags=0x08000000)
    samples=[]
    while process.poll() is None:
        try:
            sample=counts(preview)
            if not samples or samples[-1]!=sample: samples.append(sample)
        except (OSError,struct.error): pass
        time.sleep(0.025)
    assert process.returncode==0,process.stderr.read().decode(errors='replace')
    final=counts(output)
    assert samples, 'No live snapshots observed'
    assert any(0<s[0]<final[0] for s in samples), (samples,final)
    assert all(a[0]<=b[0] for a,b in zip(samples,samples[1:])),samples
    print(f'PASS: {len(samples)} live surface batches; {samples[0]} -> {final} (triangles, bodies, faces)')
finally:
    for path in [output,preview,output.with_suffix('.partial'),output.with_suffix('.status')]:
        path.unlink(missing_ok=True)
