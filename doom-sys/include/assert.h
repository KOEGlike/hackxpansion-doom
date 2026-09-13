#ifdef NDEBUG
#define assert(x) ((void)0)
#else
void dg_assert_fail(const char *file, int line);
#define assert(x) ((x) ? (void)0 : dg_assert_fail(__FILE__, __LINE__))
#endif
