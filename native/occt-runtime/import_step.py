"""Local native OCCT bridge. No network; source files are opened read-only."""
import sys, struct, math, os, time
from array import array
from OCP.STEPControl import STEPControl_Reader
from OCP.IFSelect import IFSelect_RetDone
from OCP.Interface import Interface_Static
from OCP.BRepMesh import BRepMesh_IncrementalMesh
from OCP.BRep import BRep_Tool
from OCP.BRepBndLib import BRepBndLib
from OCP.Bnd import Bnd_Box
from OCP.TopExp import TopExp_Explorer
from OCP.TopAbs import TopAbs_FACE, TopAbs_SOLID, TopAbs_SHELL, TopAbs_REVERSED
from OCP.TopoDS import TopoDS
from OCP.TopLoc import TopLoc_Location

def stage(text):
    path=os.environ.get('STEP_FAST_PROGRESS')
    if path:
        from pathlib import Path
        Path(path).with_suffix('.status').write_text(text,encoding='utf-8')

def write_mesh(output,positions,radius,faces,bodies):
    with open(output,'wb') as stream:
        stream.write(struct.pack('<Q',len(positions))); positions.tofile(stream)
        stream.write(struct.pack('<fQBQ',radius,len(bodies),False,len(faces)))
        for face in faces: stream.write(struct.pack('<II',*face))
        for text in ['mm','OpenCascade']:
            encoded=text.encode();stream.write(struct.pack('<Q',len(encoded)));stream.write(encoded)
        stream.write(struct.pack('<Q',len(bodies)))
        for body in bodies: stream.write(struct.pack('<II',*body))

def publish(positions,radius,faces,bodies):
    path=os.environ.get('STEP_FAST_PROGRESS')
    if not path: return
    from pathlib import Path
    temp=str(Path(path).with_suffix('.partial'))
    try:
        write_mesh(temp,positions,radius,faces,bodies)
        os.replace(temp,path)
    except OSError: pass

def main(source, output, quality):
    reader = STEPControl_Reader()
    Interface_Static.SetCVal_s('xstep.cascade.unit', 'MM')
    if reader.ReadFile(source) != IFSelect_RetDone:
        raise ValueError('STEP-filen kunde inte läsas')
    if reader.TransferRoots() == 0:
        raise ValueError('Inga STEP-rötter kunde importeras')
    shape = reader.OneShape()
    box = Bnd_Box(); BRepBndLib.Add_s(shape, box)
    bounds = box.Get()
    center = [(bounds[i]+bounds[i+3])*0.5 for i in range(3)]
    radius = math.sqrt(sum((bounds[i+3]-bounds[i])**2 for i in range(3)))*0.5
    positions = array('f'); faces = []; bodies = []
    groups = []
    for kind, avoid in [(TopAbs_SOLID, None), (TopAbs_SHELL, TopAbs_SOLID), (TopAbs_FACE, TopAbs_SHELL)]:
        explorer = TopExp_Explorer(shape, kind) if avoid is None else TopExp_Explorer(shape, kind, avoid)
        while explorer.More():
            groups.append(explorer.Current()); explorer.Next()
    last_publish=0.0
    for body_index, body in enumerate(groups):
        stage(f'OpenCascade - triangulerar kropp {body_index+1}/{len(groups)}...')
        mesher = BRepMesh_IncrementalMesh(body, max(radius*(0.0002 if quality=='fine' else 0.001), 1e-5), False, 0.15 if quality=='fine' else 0.35, True)
        if not mesher.IsDone(): raise ValueError('Trianguleringen misslyckades')
        body_start = len(positions)//6
        body_faces = []
        if body.ShapeType() == TopAbs_FACE: body_faces.append(body)
        else:
            explorer = TopExp_Explorer(body, TopAbs_FACE)
            while explorer.More():
                body_faces.append(explorer.Current()); explorer.Next()
        for body_face in body_faces:
            face = TopoDS.Face_s(body_face); location = TopLoc_Location()
            mesh = BRep_Tool.Triangulation_s(face, location)
            if mesh is None: raise ValueError('En yta saknar triangulering')
            transform = location.Transformation(); reverse = face.Orientation()==TopAbs_REVERSED
            nodes = [None] + [mesh.Node(i).Transformed(transform).Coord() for i in range(1,mesh.NbNodes()+1)]
            start = len(positions)//6
            for i in range(1,mesh.NbTriangles()+1):
                a,b,c = mesh.Triangle(i).Get()
                if reverse: b,c=c,b
                points=[nodes[a],nodes[b],nodes[c]]
                u=[points[1][j]-points[0][j] for j in range(3)]
                v=[points[2][j]-points[0][j] for j in range(3)]
                n=[u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]]
                length=math.sqrt(sum(x*x for x in n)); n=[x/length for x in n] if length else [0.,0.,0.]
                for p in points: positions.extend([p[j]-center[j] for j in range(3)]+n)
            faces.append((start,len(positions)//6-start))
            if time.monotonic()-last_publish>=0.12:
                publish(positions,radius,faces,bodies+[(body_start,len(positions)//6-body_start)])
                last_publish=time.monotonic()
        bodies.append((body_start,len(positions)//6-body_start))
        publish(positions,radius,faces,bodies)
        last_publish=time.monotonic()
    count = len(bodies)
    if not positions: raise ValueError('Ingen geometri kunde importeras')
    write_mesh(output,positions,radius,faces,bodies)

if __name__=='__main__':
    try: main(*sys.argv[1:])
    except Exception as error:
        print(str(error),file=sys.stderr);sys.exit(1)
