export const platforms = [
  {
    id: 'netease', name: '网易云音乐', domain: 'music.163.com',
    qualityKey: 'neteaseQuality',
    qualities: [['lossless', '无损'], ['exhigh', '极高'], ['higher', '较高'], ['standard', '标准']],
  },
  {
    id: 'bilibili', name: 'Bilibili', domain: 'bilibili.com',
    qualityKey: 'bilibiliQuality',
    qualities: [['30251', 'Hi-Res 无损'], ['30280', 'AAC 高音质'], ['30232', 'AAC 中等音质'], ['30216', 'AAC 普通音质']],
  },
  {
    id: 'youtube', name: 'YouTube', domain: 'youtube.com',
    qualityKey: 'youtubeQuality',
    qualities: [['160', '约 160 kbps'], ['128', '约 128 kbps'], ['64', '约 64 kbps']],
  },
]

export function pickQuality(media, settings) {
  const platform = platforms.find(({ id }) => media.platform === id)
  const available = media.qualities
  if (!available.length || !platform) return available[0]?.id ?? ''
  const order = platform.qualities.map(([id]) => id)
  const preferredIndex = Math.max(0, order.indexOf(settings[platform.qualityKey]))
  if (platform.id === 'youtube') {
    const target = Number(order[preferredIndex])
    const variants = available.filter(({ bitrate }) => bitrate > 0)
    return variants.reduce((nearest, item) => {
      if (!nearest || Math.abs(item.bitrate - target) < Math.abs(nearest.bitrate - target)) return item
      return nearest
    }, null)?.id ?? available[0].id
  }
  return order.slice(preferredIndex).find(id => available.some(item => item.id === id))
    ?? available[0].id
}

export function durationText(seconds) {
  if (!(seconds > 0)) return ''
  const rounded = Math.floor(seconds)
  return `${Math.floor(rounded / 60)}:${String(rounded % 60).padStart(2, '0')}`
}
