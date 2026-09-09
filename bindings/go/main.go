package main

/*
#cgo CFLAGS: -I${SRCDIR}/../../native/warpigs/include
#cgo LDFLAGS: -L${SRCDIR}/../../build/warpigs -Wl,-rpath,${SRCDIR}/../../build/warpigs -lwarpigs_core -lstdc++
#include "warpigs.h"
#include <stdlib.h>
*/
import "C"

import (
	"fmt"
	"unsafe"
)

func check(code C.uint32_t) error {
	if code == C.WP_OK {
		return nil
	}
	return fmt.Errorf("native WarPigs error: %d", uint32(code))
}

func main() {
	var engine *C.wp_engine_t
	if err := check(C.wp_engine_create(256, 10000, 42, &engine)); err != nil {
		panic(err)
	}
	defer C.wp_engine_destroy(engine)

	fmt.Printf("ABI=%d configurations=%d population=%d\n",
		uint32(C.wp_engine_abi_version()),
		uint32(C.wp_engine_configuration_count()),
		uint64(C.wp_engine_population(engine)))

	for _, action := range []C.uint32_t{C.WP_PREPARE, C.WP_START, C.WP_QUARANTINE, C.WP_TERMINATE} {
		if err := check(C.wp_engine_step(engine, action)); err != nil {
			panic(err)
		}
		fmt.Printf("tick=%d population=%d\n", uint64(C.wp_engine_tick(engine)), uint64(C.wp_engine_population(engine)))
	}

	buffer := make([]C.char, 5)
	if err := check(C.wp_engine_configuration_text(engine, 0, (*C.char)(unsafe.Pointer(&buffer[0])), C.size_t(len(buffer)))); err != nil {
		panic(err)
	}
	fmt.Printf("first configuration=%s\n", C.GoString((*C.char)(unsafe.Pointer(&buffer[0]))))
}
