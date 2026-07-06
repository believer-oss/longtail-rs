/*
 * HPCDC discriminator shim — committed in-repo (CI has no ~/github/longtail
 * checkout). The reference `HPCDCDiscriminatorFromAvg` is file-static in
 * `lib/hpcdcchunker/longtail_hpcdcchunker.c` (lines 126-129) and therefore not
 * reachable through the longtail FFI, so this file exposes the **verbatim**
 * expression under an exported name for the exhaustive discriminator
 * differential.
 *
 * Verbatim source (longtail@96241fe, lib/hpcdcchunker/longtail_hpcdcchunker.c:126-129):
 *
 *     static uint32_t HPCDCDiscriminatorFromAvg(double avg)
 *     {
 *         return (uint32_t)(avg / (-1.42888852e-7*avg + 1.33237515));
 *     }
 *
 * Compiled by longtail-testkit's build.rs (feature `differential`) with FP
 * contraction disabled (-ffp-contract=off) so a host compiler cannot fuse the
 * multiply-add into an FMA and diverge from the Rust port (moot on baseline
 * x86-64, real on ARM). MSVC does not contract by default and rejects the flag,
 * so build.rs adds it only when supported.
 */
#include <stdint.h>

uint32_t longtail_shim_discriminator_from_avg(double avg)
{
    return (uint32_t)(avg / (-1.42888852e-7*avg + 1.33237515));
}
