MEMORY {
	L1 : ORIGIN = 0x0, LENGTH = {available_space}
	SMALL_LDM : ORIGIN = 0xFFB00000, LENGTH = 2K
	LARGE_LDM : ORIGIN = 0xFFB00000, LENGTH = 4K
}

__brisc_stack_size = 4K;
__ncrisc_stack_size = 2K;
__trisc0_stack_size = 2K;
__trisc1_stack_size = 2K;
__trisc2_stack_size = 2K;

SECTIONS {
	. = ALIGN(4);

	.text : {
		__firmware_start = .;

		FILL(0xff)

		*(.init*)

		*(.text*)

		KEEP(*(.eh_frame))
		*(.eh_frame_hdr)
	} > L1

	. = ALIGN(4);

	.rodata : {
		_rodata = .;

		PROVIDE(__global_pointer$ = . + 0x800);

		*(.srodata .srodata.*);
		*(.rodata .rodata.*);

		. = ALIGN(4);

		_erodata = .;
	} > L1

	.data : {
		_data = .;

		*(.sdata .sdata.*);
		*(.data .data.*);

		_edata = .;
	} > L1

	. = ALIGN(4);

	.bss (NOLOAD) : {
		_bss = .;

		*(.sbss .sbss.* .bss .bss.*);

		. = ALIGN(4);

		_ebss = .;

		. = ALIGN(16);

		__firmware_end = .;
	} > L1

	/* fake output .got section */
	/* Dynamic relocations are unsupported. This section is only used to detect
	relocatable code in the input files and raise an error if relocatable code
	is found */
	/*
	.got (INFO) : {
		KEEP(*(.got .got.*));
	}
	*/
}

. = ORIGIN(LARGE_LDM);
.  = ALIGN(16);
___brisc_stack_bottom = .;
. += __brisc_stack_size;
___brisc_stack_top = .;

. = ORIGIN(SMALL_LDM);
.  = ALIGN(16);
___ncrisc_stack_bottom = .;
. += __ncrisc_stack_size;
___ncrisc_stack_top = .;

. = ORIGIN(SMALL_LDM);
. =  ALIGN(16);
___trisc0_stack_bottom = .;
. += __trisc0_stack_size;
___trisc0_stack_top = .;

. = ORIGIN(SMALL_LDM);
. =  ALIGN(16);
___trisc1_stack_bottom = .;
. += __trisc1_stack_size;
___trisc1_stack_top = .;

. = ORIGIN(SMALL_LDM);
. =  ALIGN(16);
___trisc2_stack_bottom = .;
. += __trisc2_stack_size;
___trisc2_stack_top = .;

PROVIDE(__L1_END = ORIGIN(L1) + LENGTH(L1));
PROVIDE(__NCRISC_LDM_END = ORIGIN(SMALL_LDM) + LENGTH(SMALL_LDM));
PROVIDE(__TRISC_LDM_END = ORIGIN(SMALL_LDM) + LENGTH(SMALL_LDM));
PROVIDE(__BRISC_LDM_END = ORIGIN(LARGE_LDM) + LENGTH(LARGE_LDM));

ASSERT((__firmware_end < __L1_END), "Stack went off of the end of the L1");

/*
ASSERT(SIZEOF(.got) == 0, "
.got section detected in the input files. Dynamic relocations are not
supported. If you are linking to C code compiled using the `gcc` crate
then modify your build script to compile the C code _without_ the
-fPIC flag. See the documentation of the `gcc::Config.fpic` method for
details.");
*/
