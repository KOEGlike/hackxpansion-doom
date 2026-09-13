#ifndef DOOM_GENERIC
#define DOOM_GENERIC

#include <stdlib.h>
#include <stdint.h>

#ifndef DOOMGENERIC_RESX
#define DOOMGENERIC_RESX 640
#endif  // DOOMGENERIC_RESX

#ifndef DOOMGENERIC_RESY
#define DOOMGENERIC_RESY 400
#endif  // DOOMGENERIC_RESY


#ifdef CMAP256

typedef uint8_t pixel_t;

#else  // CMAP256

typedef uint32_t pixel_t;

#endif  // CMAP256


extern pixel_t* DG_ScreenBuffer;

#ifdef __cplusplus
extern "C" {
#endif

void doomgeneric_Create(int argc, char **argv);
void doomgeneric_Tick();


// Implement below functions for your platform
void DG_Init();
void DG_DrawFrame();
void DG_SleepMs(uint32_t ms);
uint32_t DG_GetTicksMs();
int DG_GetKey(int* pressed, unsigned char* key);
void DG_SetWindowTitle(const char * title);

// Platform v-table for C code to call Rust functions
typedef struct DgPlatform {
    void (*log_str)(const unsigned char *s, unsigned int len);
    uint32_t (*get_ticks_ms)(void);
    void (*sleep_ms)(uint32_t ms);
    void (*fatal)(const char *msg);
    void (*set_bench)(int enabled);
    void (*push_key)(int pressed, unsigned char key);
    void (*set_wad)(const unsigned char *data, unsigned int size);
    void *(*heap_alloc)(unsigned int size);
    void *(*heap_alloc_zeroed)(unsigned int size);
    void (*heap_free)(void *ptr, unsigned int size);
    unsigned int (*heap_alloc_size)(void *ptr);
} DgPlatform;

extern const DgPlatform DG_PLATFORM;

#ifdef __cplusplus
}
#endif

#endif //DOOM_GENERIC
