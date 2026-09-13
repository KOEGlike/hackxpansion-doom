/* xpanse: wad_file_class_t backed by the flash-resident WAD image.
 * Replaces w_file_stdc.c; because the file is memory mapped (XIP), lump
 * readers get zero-copy pointers directly into flash. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "w_file.h"
#include "w_wad.h"
#include "z_zone.h"

// Flash-resident WAD image (defined in dg_xpanse.c).
extern const byte *dg_wad_data;
extern unsigned int dg_wad_size;

typedef struct
{
    wad_file_t wad;
} mem_wad_file_t;

extern wad_file_class_t stdc_wad_file;

static wad_file_t *Mem_OpenFile(char *path)
{
    mem_wad_file_t *result;

    (void)path;

    result = Z_Malloc(sizeof(mem_wad_file_t), PU_STATIC, 0);
    result->wad.file_class = &stdc_wad_file;
    result->wad.mapped = (byte *) dg_wad_data;
    result->wad.length = dg_wad_size;

    return &result->wad;
}

static void Mem_CloseFile(wad_file_t *wad)
{
    Z_Free(wad);
}

/* LZ4 block-format decoder (the build script compresses large lumps with
 * lz4_flex::block::compress). Returns the number of output bytes written, or
 * 0 on malformed input. */
static int dg_lz4_decode(const byte *src, int src_len, byte *dst, int dst_len)
{
    const byte *p = src;
    const byte *src_end = src + src_len;
    byte *out = dst;
    byte *dst_end = dst + dst_len;

    while (p < src_end)
    {
        if (out == dst_end)
            break; /* filled exactly */
        unsigned int token = (unsigned int)*p++;
        unsigned int literal_len = token >> 4;
        unsigned int match_len = token & 0xf;

        if (literal_len == 15)
        {
            unsigned int extra;
            do
            {
                if (p >= src_end)
                    return 0;
                extra = (unsigned int)*p++;
                literal_len += extra;
            } while (extra == 255);
        }
        if (p + literal_len > src_end || out + literal_len > dst_end)
            return 0;
        memcpy(out, p, literal_len);
        p += literal_len;
        out += literal_len;

        if (out == dst_end)
            break; /* output complete (last sequence has no match) */
        if (p >= src_end)
            break;
        unsigned int offset = (unsigned int)p[0] | ((unsigned int)p[1] << 8);
        p += 2;
        if (match_len == 15)
        {
            unsigned int extra;
            do
            {
                if (p >= src_end)
                    return 0;
                extra = (unsigned int)*p++;
                match_len += extra;
            } while (extra == 255);
        }
        match_len += 4;
        if (offset == 0 || (size_t)offset > (size_t)(out - dst) ||
            out + match_len > dst_end)
            return 0;
        const byte *match = out - offset;
        for (unsigned int i = 0; i < match_len; i++)
            out[i] = match[i];
        out += match_len;
    }
    return (int)(out - dst);
}

static size_t Mem_Read(wad_file_t *wad, unsigned int offset,
                       void *buffer, size_t buffer_len)
{
    size_t result;

    if ((offset & 0x80000000u) != 0)
    {
        /* LZ4-compressed lump: [u32 length][lz4 block]; decompress straight
         * into the caller's buffer. */
        unsigned int src_pos = offset & 0x7fffffffu;
        unsigned int packed_len;
        memcpy(&packed_len, dg_wad_data + src_pos, 4);
        int written = dg_lz4_decode(dg_wad_data + src_pos + 4, (int)packed_len,
                                    buffer, (int)buffer_len);
        return written > 0 ? (size_t)written : 0;
    }

    if (offset >= dg_wad_size)
    {
        return 0;
    }

    result = buffer_len;

    if (offset + result > dg_wad_size)
    {
        result = dg_wad_size - offset;
    }

    memcpy(buffer, dg_wad_data + offset, result);

    return result;
}

wad_file_class_t stdc_wad_file =
{
    Mem_OpenFile,
    Mem_CloseFile,
    Mem_Read,
};
