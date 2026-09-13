#include "doomtype.h"
#include <string.h>

/* Static variables for dg_set_bench */
static int dg_bench = 0;
static unsigned int dg_bench_ms = 0;

/* Static variables for dg_set_wad */
static const unsigned char *dg_wad_data = NULL;
static unsigned int dg_wad_size = 0;

/* Fatal error handler - hooked from i_system.c */
void dg_fatal(const char *msg) {
    if (msg != NULL) {
        dg_log_str(msg, strlen(msg));
    }
    longjmp(dg_jmp_buf, 1);
}

/* Zone base for i_system.c */
byte *dg_zone_base(int *size) {
    *size = DG_ZONE_SIZE;
    return dg_zone;
}
