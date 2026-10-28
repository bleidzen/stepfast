"""Generate our vector-style icon and a Windows x64 COFF resource (no SDK needed)."""
from pathlib import Path
from PIL import Image, ImageDraw
import struct as st

root = Path(__file__).parent
im = Image.new('RGBA', (1024,1024))
d = ImageDraw.Draw(im)
d.rounded_rectangle((24,24,1000,1000), radius=220, fill='#0d182c', outline='#284969', width=16)
# A machined isometric block: bright top, teal side, deep blue side.
d.polygon([(512,176),(840,360),(512,548),(184,360)], fill='#8ce5ef')
d.polygon([(184,360),(512,548),(512,860),(184,668)], fill='#349bbf')
d.polygon([(512,548),(840,360),(840,668),(512,860)], fill='#3561a0')
# Recessed slot gives a recognisable CAD silhouette at small sizes.
d.polygon([(512,288),(660,372),(512,456),(364,372)], fill='#152d48')
d.polygon([(364,372),(512,402),(660,372),(512,456)], fill='#247089')
d.line([(184,360),(512,548),(840,360)], fill='#b3f0f5', width=12)
im = im.resize((256,256), Image.Resampling.LANCZOS)
im.save(root/'stepfast.png')
im.save(root/'stepfast.ico', sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])
(root/'stepfast.rgba').write_bytes(im.tobytes())

# Resource directories: type -> identifier -> language -> data entry.
ico=(root/'stepfast.ico').read_bytes()
count=st.unpack_from('<H',ico,4)[0]
icons=[]; group=bytearray(st.pack('<HHH',0,1,count))
for i in range(count):
    width,height,colors,reserved,planes,bits,size,offset=st.unpack_from('<BBBBHHII',ico,6+i*16)
    icons.append(ico[offset:offset+size])
    group.extend(st.pack('<BBBBHHIH',width,height,colors,reserved,planes,bits,size,i+1))
tree={3:{i+1:{1033:data} for i,data in enumerate(icons)},14:{1:{1033:bytes(group)}}}
data=bytearray(); patches=[]
def align():
    while len(data)%4: data.append(0)
def directory(node):
    align(); offset=len(data)
    data.extend(st.pack('<IIHHHH',0,0,0,0,0,len(node)))
    entries=len(data); data.extend(bytes(len(node)*8))
    for index,(key,value) in enumerate(sorted(node.items())):
        if isinstance(value,dict): target=directory(value)|0x80000000
        else:
            align(); target=len(data); data.extend(bytes(16)); align(); payload=len(data); data.extend(value)
            st.pack_into('<IIII',data,target,payload,len(value),0,0); patches.append(target)
        st.pack_into('<II',data,entries+index*8,key,target)
    return offset
directory(tree); align()
relocs=b''.join(st.pack('<IIH',p,0,3) for p in patches) # AMD64_ADDR32NB
header=st.pack('<HHIIIHH',0x8664,1,0,60+len(data)+len(relocs),1,0,0)
section=st.pack('<8sIIIIIIHHI',b'.rsrc\0\0\0',0,0,len(data),60,60+len(data),0,len(patches),0,0x40000040)
symbol=st.pack('<8sIhHBB',b'.rsrc\0\0\0',0,1,0,3,0)
(root/'stepfast-icon.obj').write_bytes(header+section+data+relocs+symbol+st.pack('<I',4))
