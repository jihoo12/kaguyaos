#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
extern bool jihoo_acpi_discover(uint64_t, uint64_t *), jihoo_acpi_madt(uint64_t,uint64_t *);
extern void jihoo_acpi_controls(uint64_t,uint64_t *);
extern uint64_t jihoo_acpi_ecam(uint64_t,uint8_t,uint8_t,uint8_t,uint16_t);
static unsigned char arena[8192];
bool kaguya_firmware_range(uint64_t base,uint64_t n) { uintptr_t start=(uintptr_t)arena; return base>=start && base<=start+sizeof arena && n<=start+sizeof arena-base; }
static void put(void *p,uint64_t value,unsigned n) { unsigned char *b=p; for (unsigned i=0;i<n;i++) b[i]=value>>(8*i); }
static void sum(unsigned char *p,unsigned n,unsigned offset) { unsigned s=0; p[offset]=0; for(unsigned i=0;i<n;i++) s+=p[i]; p[offset]=-s; }
static void table(unsigned char *p,const char *sig,unsigned n) { memcpy(p,sig,4);put(p+4,n,4);sum(p,n,9); }
int main(void) {
    unsigned char *r=arena,*x=arena+256,*m=arena+512,*f=arena+1024,*c=arena+1536;
    uint64_t out[70]={0};
    memcpy(r,"RSD PTR ",8);r[15]=2;put(r+20,36,4);put(r+24,(uintptr_t)x,8);sum(r,20,8);sum(r,36,32);
    put(x+36,(uintptr_t)m,8);put(x+44,(uintptr_t)f,8);put(x+52,(uintptr_t)c,8);table(x,"XSDT",60);
    put(m+36,0xfee00000,4);
    unsigned char entries[]={0,8,0,2,1,0,0,0, 0,8,1,4,1,0,0,0, 0,8,2,4,1,0,0,0, 0,8,3,6,0,0,0,0,
        1,12,0,0,0,0,0xc0,0xfe,0,0,0,0, 2,10,0,9,20,0,0,0,15,0, 5,12,0,0,0,0,0xe0,0xfe,0,0,0,0};
    memcpy(m+44,entries,sizeof entries);unsigned ml=44+sizeof entries;table(m,"APIC",ml);
    put(f+40,0x12340000,4);put(f+64,0x604,4);put(f+76,0x608,4);table(f,"FACP",244);
    put(c+44,0xe0000000,8);c[54]=32;c[55]=63;table(c,"MCFG",60);
    assert(jihoo_acpi_discover((uintptr_t)r,out) && out[0]==(uintptr_t)x && out[1]==(uintptr_t)m && out[2]==(uintptr_t)f && out[4]==(uintptr_t)c);
    assert(jihoo_acpi_madt((uintptr_t)m,out));assert(out[0]==0xfee00000 && out[1]==2 && out[2]==0xfec00000 && out[4]==20 && out[5]==15 && out[6]==2 && out[7]==4);
    jihoo_acpi_controls((uintptr_t)f,out);assert(out[0]==0x12340000 && out[1]==0x604 && out[2]==0x608);
    put(f+140,0x100000000,8);f[172]=1;put(f+176,0x704,8);f[208]=1;put(f+212,0x708,8);sum(f,244,9);
    jihoo_acpi_controls((uintptr_t)f,out);assert(out[0]==0x100000000 && out[1]==0x704 && out[2]==0x708);
    assert(jihoo_acpi_ecam((uintptr_t)c,33,3,2,256)==0xe011a100);
    assert(!jihoo_acpi_ecam((uintptr_t)c,31,0,0,0) && !jihoo_acpi_ecam((uintptr_t)c,64,0,0,0) && !jihoo_acpi_ecam((uintptr_t)c,32,32,0,0));
    m[45]=0;sum(m,ml,9);assert(!jihoo_acpi_madt((uintptr_t)m,out));m[45]=8;sum(m,ml,9);
    r[32]++;assert(!jihoo_acpi_discover((uintptr_t)r,out));r[32]--;
    x[9]++;assert(!jihoo_acpi_discover((uintptr_t)r,out));x[9]--;
    put(x+4,61,4);sum(x,61,9);assert(!jihoo_acpi_discover((uintptr_t)r,out));table(x,"XSDT",60);
    put(x+36,UINT64_MAX,8);sum(x,60,9);assert(jihoo_acpi_discover((uintptr_t)r,out) && !out[1]);
    assert(!jihoo_acpi_discover((uintptr_t)arena+8190,out));
    puts("jihoo ACPI: checksums, bounded mapping, MADT topology, malformed records, FADT and ECAM passed");
}
