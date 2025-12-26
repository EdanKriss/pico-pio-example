/* Memory layout for Raspberry Pi Pico (RP2040) */

MEMORY {
    /* Boot loader occupies first 256 bytes of flash */
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100

    /* Main flash storage - 2MB total, minus 256 bytes for bootloader */
    /* This is where program code lives */
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100

    /* RAM - 264KB total */
    /* This is where variables and stack live */
    RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}

/* Tell the bootloader to use the rp2040-boot2 crate */
EXTERN(BOOT2_FIRMWARE)

SECTIONS {
    .boot2 ORIGIN(BOOT2) :
    {
        KEEP(*(.boot2));
    } > BOOT2
} INSERT BEFORE .text;
