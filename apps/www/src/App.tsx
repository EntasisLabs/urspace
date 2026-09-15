import { Footer } from './components/Footer'
import { Hero } from './components/Hero'
import { HowItWorks } from './components/HowItWorks'
import { Install } from './components/Install'
import { Modes } from './components/Modes'
import { Nav } from './components/Nav'
import { Position } from './components/Position'

export default function App() {
  return (
    <div className="relative overflow-x-clip">
      <Nav />
      <main>
        <Hero />
        <HowItWorks />
        <Modes />
        <Position />
        <Install />
      </main>
      <Footer />
    </div>
  )
}
