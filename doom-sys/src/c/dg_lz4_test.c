#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "doomtype.h"

extern int dg_lz4_decode(const byte *src, int src_len, byte *dst, int dst_len);

int main(int argc, char **argv)
{
    /* usage: dg_lz4_test <compressed-file> <raw-file> */
    FILE *c = fopen(argv[1], "rb");
    FILE *r = fopen(argv[2], "rb");
    fseek(c, 0, SEEK_END); int clen = ftell(c); fseek(c, 0, SEEK_SET);
    fseek(r, SEEK_SET == 0 ? 0 : 0, SEEK_END); int rlen = ftell(r); fseek(r, 0, SEEK_SET);
    byte *src = malloc(clen); byte *dst = malloc(rlen + 1024);
    fread(src, 1, clen, c); fread(dst, 1, rlen, r);
    byte *decoded = malloc(rlen + 1024);
    memset(decoded, 0xEE, rlen + 1024);
    int n = dg_lz4_decode(src, clen, decoded, rlen);
    if (n != rlen)
    {
        printf("FAIL: decoded %d of %d\n", n, rlen);
        return 1;
    }
    if (memcmp(decoded, dst, rlen) != 0)
    {
        printf("FAIL: mismatch\n");
        for (int i = 0; i < rlen; i++)
        {
            if (decoded[i] != dst[i]) { printf("first diff at %d\n", i); break; }
        }
        return 1;
    }
    printf("OK %d bytes\n", rlen);
    return 0;
}
/* stand-ins for the globals dg_wadfile.c references */
const byte *dg_wad_data = NULL;
unsigned int dg_wad_size = 0;
