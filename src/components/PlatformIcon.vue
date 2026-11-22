<script setup>
import NeteaseCloudMusic from '~icons/simple-icons/neteasecloudmusic'
import Bilibili from '~icons/simple-icons/bilibili'
import Youtube from '~icons/simple-icons/youtube'

defineProps({ platform: String })

const icons = {
  netease: NeteaseCloudMusic,
  bilibili: Bilibili,
  youtube: Youtube,
}
</script>

<template>
  <span class="platform-icon" :class="platform" aria-hidden="true">
    <component :is="icons[platform]" :width="22" :height="22" />
  </span>
</template>
