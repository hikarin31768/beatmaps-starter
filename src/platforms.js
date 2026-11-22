export const platforms = [
  {
    id: 'netease', name: '网易云音乐', domain: 'music.163.com',
    qualityKey: 'neteaseQuality',
    qualities: [['hires', 'Hi-Res'], ['lossless', '无损'], ['exhigh', '极高'], ['higher', '较高'], ['standard', '标准']],
  },
  {
    id: 'bilibili', name: 'Bilibili', domain: 'bilibili.com',
    qualityKey: 'bilibiliQuality',
    qualities: [['30251', 'Hi-Res 无损'], ['30250', '杜比全景声'], ['30280', 'AAC 高音质'], ['30232', 'AAC 中等音质'], ['30216', 'AAC 普通音质']],
  },
  {
    id: 'youtube', name: 'YouTube', domain: 'youtube.com',
    qualityKey: 'youtubeQuality',
    qualities: [['160', '约 160 kbps'], ['128', '约 128 kbps'], ['64', '约 64 kbps']],
  },
]

export function pickQuality(media, settings) {
  const platform = platforms.find(({ id }) => media.platform === id)
  const preference = settings[platform.qualityKey]
  const available = media.qualities
  if (!available.length || preference === 'best') return available[0]?.id ?? ''
  if (platform.id === 'youtube') {
    const variants = available.filter(({ bitrate }) => bitrate > 0)
    return variants.reduce((nearest, item) => {
      if (!nearest || Math.abs(item.bitrate - Number(preference)) < Math.abs(nearest.bitrate - Number(preference))) return item
      return nearest
    }, null)?.id ?? available[0].id
  }
  const order = platform.qualities.map(([id]) => id)
  const target = order.indexOf(preference)
  return available.find(({ id }) => id === preference)?.id
    ?? order.slice(target + 1).find(id => available.some(item => item.id === id))
    ?? available[0].id
}

export function durationText(seconds) {
  if (!(seconds > 0)) return ''
  const rounded = Math.floor(seconds)
  return `${Math.floor(rounded / 60)}:${String(rounded % 60).padStart(2, '0')}`
}
